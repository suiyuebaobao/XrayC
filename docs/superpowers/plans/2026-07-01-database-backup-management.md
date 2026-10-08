# 数据库备份管理 · 实现计划

> **For agentic workers:** 用 subagent-driven-development 逐 task 实现(TDD)。依据 spec `docs/superpowers/specs/2026-07-01-database-backup-management-design.md`。

**Goal:** 后台「数据库备份」页:配异地机自动装公钥、多模式(全量/WAL/邮件)可组合可单选、自定义调度(间隔+cron)、全程脱敏。

**Architecture:** 配置存 `site_settings.backup_config`(明文+写时保留+脱敏,字段加密已下线);worker 循环编排各模式;共享 crate `xrayc-backup` 收敛备份逻辑;pgBackRest 做 WAL/PITR;rsync 异地增量。

**Tech Stack:** Rust(worker/db/api/新 crate)、Vue3 前端、pgBackRest、rsync、openssl、lettre、cron crate。

---

## 阶段与 Task(先易后难;标注可并行组)

### Phase 1 — 数据模型 + 配置读写(基础,阻塞其它)
**Task 1:** `backup_config` setting_key:default/规范化/脱敏(`_set`)/写时保留 + store 读写。
- Files: `crates/db/src/settings_json.rs`(+default_backup_config/normalize/public_backup_config)、`crates/db/src/store/settings.rs`(+backup_config_json/update_backup_config_json)、`crates/db/src/store/json_util.rs`(写时保留路径加 ssh_password/attach_passphrase)、`crates/db/src/types.rs`(+BackupConfig 结构可选)。
- Test: round-trip 保留旧密码不被 `***` 覆盖;public 版把密码出成 `_set`;默认值。
- 验收:`cargo test -p xrayc-db` 过;明文存、输出脱敏。

### Phase 2 — backups.rs 拆模块 + 调度(基础)
**Task 2:** 把 `crates/worker/src/backups.rs`(509行)拆成 `backups/{mod,schedule,dump,secrets}.rs`(offsite/wal/email 先留空壳),行为零变化,守 550 行。
- Test: 现有 backups 单测全过(迁移到新模块)。
**Task 3:** `backups/schedule.rs`:`ScheduleKind{Interval(minutes),Cron(expr),AfterFull}`;`is_due(kind,last_success,now)` 纯函数;引入 `cron` crate。
- Test: 间隔边界、cron 触发点、after_full 语义。
- 验收:`cargo test -p xrayc-worker` schedule 全过。

### Phase 3 — 全量增强(依赖 T1/T2)
**Task 4:** `backups/dump.rs`:`exclude_traffic_logs` 追加 `--exclude-table-data=`(spec §5.1 清单);读 backup_config.full 驱动;调度换 schedule.rs。
- Test: 命令组装含 7 张排除表、必留表不排;due 判定用新 schedule。

### Phase 4 — 共享 crate + 邮件(可与 P5 并行,独立 worktree)
**Task 5:** 新建 `crates/backup`(`xrayc-backup`):下沉 SMTP 发送(支持 multipart 附件)+ provision/offsite/wal 逻辑入口;`crates/api/src/guards.rs` 改调共享 SMTP。
- Files: 新 crate + workspace Cargo.toml + api guards 改。
- Test: 发送函数单测(mock SMTP 或纯构造 multipart 断言)。
**Task 6:** `backups/email.rs`:附件 openssl 加密 + 超限降级 + 成功/失败通知,调 xrayc-backup SMTP。
- Test: 加密命令组装、超限降级分支、通知文案脱敏。

### Phase 5 — 异地 provision + rsync(独立 worktree,并行 P4)
**Task 7:** `backups/offsite.rs`:`provision_offsite()`(sshpass 装公钥+校验免密+置位)、`sync_offsite()`(rsync 密钥免密增量)、异地保留清理;私钥自动生成到卷路径。
- Test: 命令组装(rsync/ssh 参数)、脱敏、provision 结果解析(纯函数部分)。

### Phase 6 — WAL/pgBackRest(最难,依赖镜像;隔离测)
**Task 8:** `backups/wal.rs`:pgBackRest 封装(stanza-create/backup/expire/状态);archive_command 脚本;配置生成。
- Test: 命令/配置组装单测;**隔离 postgres 集成**:archive+base backup+PITR restore 演练(见 Task 13)。

### Phase 7 — API + 前端(依赖 P1;前端可后并行)
**Task 9:** `crates/api/src/routes/admin_backup.rs`:GET/PUT config(脱敏)、test-and-provision、run-now、state。接线路由 + admin JWT。
- Test: axum-test:GET 脱敏、PUT 写时保留、鉴权。
**Task 10:** 前端「数据库备份」页(`frontend/src/views/admin/...`):配置表单 + 各模式开关 + 调度(间隔/cron)+ 测试连通按钮 + 状态历史。守 550 行(拆子组件)。
- Test: 前端 build + 组件单测(如有)+ e2e 冒烟。

### Phase 8 — 编排接线(依赖 P2-P8)
**Task 11:** `backups/mod.rs`:`run_all_if_due()` 按各模式 schedule 顺序跑 full→offsite→wal→email,状态汇总;`worker/main.rs` 调用点从 `run_if_due` 换 `run_all_if_due`。
- Test: 编排单测(各模式 enabled 组合)。

### Phase 9 — 部署 + 镜像 + 文档(独立 worktree)
**Task 12:** `Dockerfile`(worker 加 `rsync pgbackrest`)、`docker-compose.yml`(worker 加卷 wal-archive/pgbackrest-repo/backup-ssh;postgres 加 archive command + wal-archive 卷,先 `/bin/true` 兜底)、`scripts/lib/install/*` 同步。
**Task 14(文档):** `开发方案.md` 新章节 + §13 改;`AGENTS.md`+`CLAUDE.md` §13 同步;`文档/运维手册/备份恢复.md`(多模式+恢复+演练+明文提示);`文档/部署/部署说明.md`;`CHANGELOG.md`;`文档/接口/管理接口.md`。`scripts/check-docs-no-stale.sh` 过。

### Phase 10 — 验证(主 agent 亲自)
**Task 13(恢复演练+隔离WAL):** 隔离 postgres 容器验证:全量 dump+pg_restore 恢复;WAL archive + `pg_basebackup` + PITR restore 到指定时刻。脚本化。
> **✅ 2026-07-01 已隔离实证通过(plain WAL,无 pgBackRest)——配方直接照用:**
> - postgres 起容器带 `-c wal_level=replica -c archive_mode=on -c "archive_command=test ! -f /wal-archive/%f && cp %p /wal-archive/%f" -c max_wal_senders=3`,挂 `/wal-archive` 卷;
> - base:`pg_basebackup -U <user> -D <repo>/base -X stream -c fast`(走网络,不需 PGDATA 本地访问);
> - 恢复:`cp -a base restore` → 建 `recovery.signal` → auto.conf 写 `restore_command='cp /wal-archive/%f %p'` + `recovery_target_time='<T>'` + `recovery_target_action='promote'` + `archive_mode=off` → 起容器挂 restore 为 PGDATA + `/wal-archive:ro`;
> - 实证结果:恢复日志 `recovery stopping before commit ... time <T之后>`,目标后的事务被正确排除。
**Task 15(真机):** 配那台异地机(指纹 35ef816a)→ test-and-provision 装公钥 → 全量+异地同步+邮件各跑通 → 抽查异地有文件/邮件收到加密附件/计费表在 dump。
**Task 16(门禁):** `make check`(fmt/clippy/test/文件长度/文档不陈旧)、前端 build 全过。

## 风险闸(spec §9/§10)
- WAL live 上线:隔离测通 → compose 先 `/bin/true` 确认 postgres 能起 → 切实际 archive_command → 失败即 `git checkout docker-compose.yml && up -d postgres` 回滚。**不拿 live 库冒险,先隔离。**
- 并行改文件的 subagent 各自独立 worktree(P4/P5/P9 可并行)。

## 交付
一版全交付。实现顺序:P1→P2→P3→(P4∥P5)→P6→P7→P8→P9→P10。主 agent 每 task 两段复核(spec 符合 + 代码质量),全绿再下一步。
