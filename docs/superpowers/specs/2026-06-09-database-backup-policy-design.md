# 数据库自动备份策略设计

## 目标

管理平台可以配置 PostgreSQL 自动备份机制，默认启用，1 天备份一次，备份文件默认保留 30 天。该机制由中心 Worker 执行，不依赖管理员手工 cron。

## 配置模型

运营设置 `site_settings.setting_key=access_operations` 新增：

```json
{
  "database_backup": {
    "enabled": true,
    "interval_days": 1,
    "retention_days": 30
  }
}
```

字段范围：

- `enabled`：布尔值，默认 `true`。
- `interval_days`：整数，范围 1 到 30，默认 1。
- `retention_days`：整数，范围 1 到 3650，默认 30。

服务端保存时统一归一化和裁剪范围。前端读取 snake_case 或 camelCase，提交时使用 snake_case。

## 执行模型

Worker 每轮维护任务后读取 `database_backup`。如果启用且距离 `database_backup_state.last_success_at` 已达到间隔，则获取 PostgreSQL advisory lock，锁内再次判断是否到期，然后执行备份。

备份命令：

```bash
PGHOST=<host> PGPORT=<port> PGDATABASE=<db> PGUSER=<user> PGPASSFILE=<0600-pgpass> \
  pg_dump --format=custom --no-owner --no-acl --file <backup-file>
```

备份目录由 `DATABASE_BACKUP_DIR` 或 `XRAYC_DATABASE_BACKUP_DIR` 指定，默认 `/backups`。文件名格式为 `xrayc-postgres-YYYYmmddTHHMMSSZ.dump`。

执行保护：

- `DATABASE_URL` 不作为 `pg_dump` 命令行参数传入，避免数据库密码出现在进程参数中。
- `DATABASE_BACKUP_TIMEOUT_SECONDS` 控制单次 `pg_dump` 超时，默认 1800 秒，范围 60 到 86400。
- `DATABASE_BACKUP_RETRY_SECONDS` 控制失败后重试退避，默认 3600 秒，范围 60 到 86400。

## 状态与清理

Worker 写入 `site_settings.setting_key=database_backup_state`：

- `status`
- `last_started_at`
- `last_success_at`
- `last_failed_at`
- `last_file_name`
- `backup_dir`
- `error_summary`

清理只删除备份目录下符合 `xrayc-postgres-*.dump` 命名且超过 `retention_days` 的普通文件，避免误删目录内其他文件。

## 边界

自动备份只覆盖 PostgreSQL。环境变量、字段加密 keyring provider、部署产物、Caddy/TLS 配置和服务器级快照仍需单独备份。

恢复仍使用运维手册流程：

```bash
pg_restore --clean --if-exists --no-owner --dbname "$DATABASE_URL" backups/xrayc-postgres-YYYYmmddTHHMMSSZ.dump
```
