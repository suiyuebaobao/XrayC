# 数据库备份管理 · 设计文档(Spec)

> 日期:2026-07-01。作者:自主设计(用户 2026-07-01 授权全做、不分批)。
> 状态:已定稿,待实现。上游依据:`开发方案.md`(最高)、`文档/运维手册/备份恢复.md`、`crates/worker/src/backups.rs` 现有实现。

## 1. 目标与范围

后台新增「数据库备份」管理页:管理员填异地备份机连接信息(IP/SSH),系统**自动 SSH 去装公钥并配置**;支持**多种备份方式(全量 / WAL 真增量 / 邮件发文件)可组合、可单选、独立开关**;**调度自定义**(间隔 + cron 两种);全程安全(脱敏、不入日志)。

**不在范围**:多数据库/多实例、跨云容灾编排、备份文件浏览器 UI(仅列表+状态)、恢复的一键 UI(恢复走运维手册命令 + 演练脚本)。

## 2. 关键约束与现状(探查结论)

- **字段级加密已下线**(`crates/db/src/store/auth_challenges.rs:34`、`开发方案.md:338/1351`):SMTP 密码、支付密钥均**明文存 `site_settings`**,靠"写时保留 `preserve_write_only_at`(`json_util.rs:55`)+ 输出脱敏成 `_set` 布尔(`settings_payment_json.rs:60`)+ 审计不记明文"防护。
  → **本功能的 SSH 密码 / 邮件附件口令,照此口径:明文存 + 写时保留 + 输出脱敏 + 绝不入日志/审计/订阅/前端回显。**(不再声称"字段加密"。)
- **§13 冲突**:§13 原文"SSH 密码/私钥绝不落库"。用户 2026-07-01 明确决定:**备份目标 SSH 密码可落库作兜底**(密钥失效可回退重装公钥)。→ 本功能改 §13,同步 `开发方案.md`/`AGENTS.md`/`CLAUDE.md`。
- **设置读写**:`site_setting_json(key,default)` / `upsert_site_setting_json(key,value)`(`crates/db/src/store/settings.rs:157/172`)。备份策略现从 `access_operations.database_backup` 读(`settings_json.rs:394`),`DatabaseBackupPolicy{enabled,interval_days,retention_days}`(`types.rs:557`),状态 `database_backup_state_json`/`update_*`(`settings.rs:132/137`),并发锁 `try_database_backup_lock()` advisory `202606090001`(`settings.rs:142`)。
- **worker 循环**:`crates/worker/src/main.rs:43-66`,tick 默认 60s(`WORKER_TICK_SECONDS`),顺序执行维护→部署卡死清理→`backup_runner.run_if_due()`。判 due 仅天级(`backups.rs:258-276`)。
- **邮件**:`send_email_code_via_smtp()`(`crates/api/src/guards.rs:187-250`,lettre,**纯文本无附件**)。SMTP 配置在 `auth_security.email_verification`(密码明文+写时保留)。
- **部署**:`docker-compose.yml`(仓库根)。postgres `postgres:16-alpine` 无自定义配置。worker 镜像(`Dockerfile`)含 `pg_dump/pg_restore`(从 `postgres:16-bookworm` 拷)+ `openssh-client/sshpass`,**无 `rsync`/`pgbackrest`**。worker 卷仅 `./backups:/backups`。

## 3. 安全模型

| 敏感项 | 存储 | 防护 |
|---|---|---|
| 异地机 SSH 密码 | 明文 `site_settings.backup_config.offsite.ssh_password` | 写时保留 + 输出脱敏 `ssh_password_set:bool` + 绝不入日志/审计;仅"装公钥/密钥失效回退"时读用 |
| 邮件附件口令 | 明文 `site_settings.backup_config.email.attach_passphrase` | 同上,脱敏 `attach_passphrase_set:bool` |
| 中心机私钥 | 文件(worker 容器 `/var/lib/xrayc/backup_ssh/id_ed25519`,0600,挂卷持久) | 不入库、不入日志、不外发;公钥可展示指纹 |

- 日常同步走**密钥免密**;SSH 密码只作"密钥失效自愈"兜底。
- 复用现有脱敏 `sanitize_text()`(`backups.rs:433`):任何命令输出、错误摘要都过脱敏(去密码/连接串/host)。
- **诚实提示(写运维手册)**:SSH 密码 / 邮件口令明文入库 → 会随 dump 进异地/邮件(与 SMTP/支付密钥现状一致)。邮件模式发出的 dump **应含全部业务数据但邮件传输仅加密附件保护**;不额外从 dump 剔除 backup_config(保持 dump 完整可恢复)。

## 4. 数据模型

新增 setting_key **`backup_config`**(与现有 `access_operations.database_backup` 策略解耦;后者保留兼容,新页统一读写 `backup_config`;首版迁移把旧 `database_backup` 的 enabled/retention 读入 `backup_config.full` 默认)。

```jsonc
// site_settings.setting_key = 'backup_config'
{
  "offsite": {                       // 异地服务器(rsync/pgBackRest 目标)
    "enabled": false,
    "ssh_host": "",                  // IP(§13:一键类操作必须 IP)
    "ssh_port": 22,
    "ssh_user": "root",
    "ssh_password": "",              // 明文兜底,输出脱敏成 ssh_password_set
    "remote_dir": "/var/backups/xrayc",
    "pubkey_installed": false,       // 公钥是否已装(装成功置 true)
    "pubkey_fingerprint": "",        // 展示用
    "retention_days": 30
  },
  "full": {                          // 全量 pg_dump
    "enabled": true,
    "exclude_traffic_logs": false,   // 排除观测大表(见 §5.1 清单)
    "schedule": { "kind": "interval", "interval_minutes": 1440, "cron": "" },
    "retention_days": 30
  },
  "wal": {                           // WAL/PITR(pgBackRest)
    "enabled": false,
    "configured": false,             // pgBackRest+archive_mode 是否就绪
    "full_backup_schedule": { "kind": "interval", "interval_minutes": 10080, "cron": "" },
    "retention_full": 4              // 保留几个 base backup(pgBackRest retention-full)
  },
  "email": {                         // 邮件发文件
    "enabled": false,
    "recipients": [],
    "attach_encrypted": true,
    "attach_passphrase": "",         // 明文兜底,脱敏成 attach_passphrase_set
    "max_attach_mb": 20,             // 超限降级:改发精简 dump 或仅通知
    "notify_on_success": true,
    "notify_on_failure": true
  }
}
```

状态扩展 `database_backup_state`(沿用现键):每模式各记 `last_started_at/last_success_at/last_failed_at/error_summary/last_file_name`,新增 `offsite_last_sync_at/wal_last_archive_ok/email_last_sent_at`。

## 5. 备份方式(可组合可单选)

### 5.1 全量 pg_dump(增强现有)
- 沿用 `pg_dump --format=custom --no-owner --no-acl`;`exclude_traffic_logs=true` 时追加 `--exclude-table-data=` 覆盖:`access_exit_probes access_exit_probe_states access_line_probes access_line_metric_snapshots node_runtime_metrics access_user_session_events subscription_pull_events`(**保结构、只删数据**;`usage_ledgers/access_traffic_snapshots/usage_*_rollups/user_subscriptions` 必留)。
- 调度改自定义(见 §6)。保留期按 `full.retention_days` 本地清理(现有 `prune_old_backup_files`)。

### 5.2 异地同步(rsync 增量)
- worker 内新增 `OffsiteSyncRunner`:每次全量备份成功后(或独立调度)`rsync -az --partial` 把 `/backups/` 增量推到 `offsite.ssh_user@ssh_host:remote_dir`,走 SSH **密钥免密**(`-e "ssh -i <私钥> -o StrictHostKeyChecking=accept-new"`)。
- 只搬新文件(rsync 天然增量);异地保留期由 worker 侧 `ssh ... find remote_dir -mtime +N -delete`(仅限文件名前缀 `xrayc-postgres-`,双重约束防误删)。
- **失败不阻塞**:异地不可达只记状态+告警,不影响本地备份与 postgres。

### 5.3 WAL / PITR
> **架构修正(2026-07-01 设计中发现)**:pgBackRest 的 `backup` 命令需本地访问 postgres 数据目录(PGDATA),但本项目 postgres 独立容器、worker 另一容器,worker 里 pgbackrest 读不到 PGDATA;要么把 pgbackrest 塞进自定义 postgres 镜像(backup 由容器内 cron 跑→面板难控调度),要么搞多主机 repo-host SSH(复杂)。
> **改用 plain WAL 归档 + `pg_basebackup`**(更契合容器架构 + 面板可控调度):
- postgres 开 `archive_mode=on`、`wal_level=replica`、`archive_command` 把 WAL 段 `cp/test` 到**本地归档卷** `/var/lib/postgresql/wal-archive`(**永不等网络,防爆盘**;archive_command 在 postgres 容器内,只写本地卷);
- **base backup 由 worker 跑 `pg_basebackup`**(走**网络复制协议**连 postgres,不需 PGDATA 本地访问;需 `max_wal_senders>0`,PG16 默认 10;replication 权限),按 `wal.full_backup_schedule` 周期做,落 worker 可读的仓库卷;
- WAL 归档卷(postgres 写)+ base 备份(worker 写)一并 rsync 推异地;
- **保留**:worker 侧脚本删过期 base + 早于最老保留 base 的 WAL(协调保留,单测覆盖边界,防删错);
- **PITR 恢复**:restore base 到新数据目录 + `recovery_target_time` + `restore_command` 从归档取 WAL 重放到指定时刻(隔离库演练,Task 13)。
- **(Task 8/13 隔离 postgres 先原型验证此路径全通,再定实现;若 plain WAL 有坑再回退评估 pgbackrest-in-custom-image。)**
- **开启需一次 postgres 重启**(archive_mode 改了才生效)——见 §9 风险与 §10 上线步骤(隔离测试→受控上线→可回滚)。
- **降级**:pgBackRest/archive 不可用时,WAL 模式标 `configured=false` 并告警,**绝不阻塞全量+同步**,更不拖垮 postgres。

### 5.4 邮件发文件
- 备份成功后,若 `email.enabled`:取最新 dump(或 `exclude_traffic_logs` 的精简版)→ 若 `attach_encrypted` 用 `openssl enc -aes-256-cbc -pbkdf2 -pass pass:<passphrase>`(镜像已有 openssl)加密 → 作附件经 SMTP 发 `recipients`。
- **超 `max_attach_mb` 降级**:改发"精简 dump"(排流量日志);再超则**只发通知说"备份成功但过大未附,请从异地取"**(不静默)。
- **通知邮件**:`notify_on_success/failure` 控制,发纯文本(成功:时间/大小/文件名脱敏;失败:脱敏错误摘要)。

## 6. 调度(间隔 + cron)

- `schedule.kind ∈ {interval, cron}`;`interval` 用 `interval_minutes`(≥1,取代旧 `interval_days`),`cron` 用 5 段表达式。
- 改 `backups.rs` 判 due:
  - `interval`:`now >= last_success_at + interval_minutes`(替换 `Duration::days`);
  - `cron`:引入轻量 cron 库(`cron` crate,已在生态、无重依赖),算"上次触发点",`last_success_at < 上次触发点 <= now` 即 due。
- worker tick 60s 已足够支撑分钟级;失败重试 `next_attempt_at` 逻辑保留。
- 各模式(full / wal base / offsite / email)可各自独立 schedule;offsite/email 也可"跟随 full 成功即触发"(`kind:"after_full"`)。

## 7. SSH 自动配置(装公钥)

API `POST /api/admin/backup/test-and-provision`(§13 半自动、凭据即用即弃**但本功能例外落库**):
1. 校验 `ssh_host` 是 IP(§13:域名尤其 CF 无法直连 SSH,拒域名)。
2. worker/api 侧(**决定放 worker**:它持私钥卷、跑同步):首启生成中心机密钥对(若无);
3. 用页面填的 SSH 密码 `sshpass ssh` 进异地机 → `mkdir -p ~/.ssh; 追加公钥到 authorized_keys(去重)` → 立即用**密钥**验证免密成功;
4. 成功:置 `pubkey_installed=true`、记 `pubkey_fingerprint`、SSH 密码按 §3 落库(兜底);失败:返回脱敏原因,不置位。
- api 不直接 SSH(§13 精神:api 不亲自 SSH)。**定案:provision/同步逻辑收敛到共享 crate `xrayc-backup`(worker 与 api 都可依赖),api 的 `test-and-provision` 端点调用该 crate 的 `provision_offsite()` 在 **api 进程内**执行一次性 SSH(装公钥),与"一键装 agent"由 api 临时 SSH 同构(§13 已允许 api 临时 SSH 装 agent、用完即弃;本功能额外把密码落库作兜底)。日常定时同步由 worker 跑。私钥文件放持久卷,worker(读写)+ api(读)都挂。不引入 deployment_tasks 跨进程编排。

## 8. 组件拆分(文件级,守 550 行硬上限)

`crates/worker/src/backups.rs`(现 509 行)**必拆**为 `backups/` 模块:
- `backups/mod.rs`:编排(run_all_if_due:按各模式 schedule 依次跑 full→offsite→wal→email),锁,状态。
- `backups/schedule.rs`:间隔/cron 判 due(纯函数 + 单测)。
- `backups/dump.rs`:全量 pg_dump(现 create_dump/prune + exclude-table-data)。
- `backups/offsite.rs`:rsync 同步 + 异地保留 + provision(装公钥)。
- `backups/wal.rs`:pgBackRest 封装(stanza/backup/expire/状态)。
- `backups/email.rs`:附件加密 + 发送(调共享 SMTP)。
- `backups/secrets.rs`:脱敏(现 sanitize_*)。

其它:
- `crates/db`:`backup_config` 读写(default/规范化/脱敏/写时保留)+ store 函数;`types.rs` 加 `BackupConfig` 结构;可能迁移(仅 site_settings JSON,无需新表)。
- `crates/core`(或新 `crates/backup`):共享给 api+worker 的 SMTP 发送(从 `api/guards.rs` 下沉 `send_email` 支持附件/multipart)+ provision 逻辑。
- `crates/api`:`admin/backup` 接口(见 §11);`guards.rs` SMTP 改调下沉后的共享函数。
- `frontend`:`views/admin` 新增「数据库备份」页。
- `deploy`:`Dockerfile`(worker 加 `rsync pgbackrest`)、`docker-compose.yml`(worker 加卷 `./wal-archive:/wal-archive`、`./pgbackrest-repo:/pgbackrest-repo`、`./backup-ssh:/var/lib/xrayc/backup_ssh`;postgres 加 `command` archive 参数 + `./wal-archive:/var/lib/postgresql/wal-archive`)、`scripts/lib/install/*` 同步。

## 9. 风险与防护

| 风险 | 防护 |
|---|---|
| WAL 归档失败撑爆 postgres 盘 | archive_command 只写**本地卷**(永不等网络);盘用量监控告警;pgBackRest 独立 expire |
| 改 archive_mode 需重启 postgres,live 库停机/起不来 | **隔离 postgres 先测通配置**;live 上线前备份 compose,`archive_command='/bin/true'` 兜底能起,再切实际;失败即回滚 compose 恢复 |
| SSH 密码明文入库/进 dump | 与 SMTP/支付现状一致;脱敏输出+不入日志;运维手册明示 |
| rsync/pgbackrest 镜像缺失 | Dockerfile 补装;缺失时模式标未就绪+告警,不崩 |
| 邮件发大附件失败 | 超限降级(精简/仅通知),不静默 |
| 恢复没演练=用不了 | 交付含**恢复演练脚本**,隔离库跑通全量恢复 + WAL PITR |

## 10. WAL 上线步骤(受控、可回滚)

1. 隔离 postgres 容器验证 `command` archive 参数 + pgBackRest `stanza-create/backup/restore` 全通(§9 §12)。
2. Dockerfile 加 rsync+pgbackrest,rebuild worker 镜像验证工具就位。
3. `docker-compose.yml` 加卷 + postgres archive command(先 `archive_command='/bin/true'` 确认能起),`docker compose up -d postgres` 观察健康。
4. 切实际 archive_command(pgbackrest archive-push)→ `stanza-create` → 首次 base backup → 验证 archive 正常、盘稳定。
5. 全程失败即 `git checkout docker-compose.yml && docker compose up -d postgres` 回滚。

## 11. API(admin)

- `GET  /api/admin/backup/config` → 脱敏配置(密码/口令出 `_set`)。
- `PUT  /api/admin/backup/config` → 写(写时保留密码/口令)。
- `POST /api/admin/backup/test-and-provision` → 测 SSH 连通 + 装公钥(§7),返回脱敏结果 + 置 pubkey_installed。
- `POST /api/admin/backup/run-now?mode=full|offsite|wal|email` → 立即触发一次(排队给 worker / 直接执行)。
- `GET  /api/admin/backup/state` → 各模式最近状态/历史。
- 鉴权:管理员 JWT(`/api/admin/*`)。所有响应脱敏。

## 12. 测试与门禁

- **单测**:`schedule.rs`(间隔/cron 判 due 边界)、exclude-table-data 组装、脱敏、附件加密、配置脱敏/写时保留。
- **集成(隔离 PostgreSQL)**:backup_config 读写 round-trip;全量 dump+恢复;WAL 隔离库 archive+PITR restore 演练。
- **真机**:配那台异地机(指纹 35ef816a)→ test-and-provision 装公钥 → 全量+异地同步+邮件各跑通 → 抽查异地有文件、邮件收到加密附件、计费表在 dump 里。
- **门禁**:`make check`(fmt/clippy/test/文件长度/文档不陈旧)、前端 build;WAL 部分隔离验证 + 受控上线。
- **禁**:对 live 发布库做破坏性测试;WAL 先隔离验证再受控上线。

## 13. 文档同步(§0)

- `开发方案.md`:新增「数据库备份管理」章节(架构/模式/调度/安全/WAL 上线);§13 改"备份目标 SSH 密码可落库作兜底(明文+脱敏,因字段加密已下线)"。
- `AGENTS.md` + `CLAUDE.md`:§13 对应条同步(双向一致)。
- `文档/运维手册/备份恢复.md`:多模式说明 + 恢复(全量 pg_restore + WAL pgBackRest PITR)+ 演练 + 明文凭据提示。
- `文档/部署/部署说明.md` + `scripts/lib/install/*`:新卷/镜像工具/postgres archive。
- `CHANGELOG.md`、`文档/接口/管理接口.md`:新接口。

## 14. 交付顺序(功能全做,一次交付)

一版实现全部;实现内部先易后难:数据模型+配置读写 → 全量增强+调度 → 异地 provision+rsync → 邮件 → WAL/pgBackRest(隔离测) → 前端页 → 部署/镜像/文档 → 真机验证 + 恢复演练。
