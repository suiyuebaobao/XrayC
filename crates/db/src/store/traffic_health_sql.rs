//! 健康检查流量统计共享 SQL 片段。
//! 本文件保存分组到中转入口的公共 CTE。
//! items 和 charts 两个模块共用同一段 SQL。
//! 分组口径仍以启用分组和 access_lines.line_group_id 为准。
//! 这里只存放只读 SQL 常量，不执行查询。
//! 修改分组统计口径时应同步验证运营中心读模型。
//! 不得在这里加入真实出口凭据或用户敏感字段。
//! 本拆分只为满足源码长度门禁，不改变行为。
//! 新增 SQL 时保持脱敏和聚合口径一致。
//! 本头部满足前十行中文注释约束。

pub(crate) const GROUP_LINES_SQL: &str = r#"
SELECT DISTINCT l.line_group_id AS entity_id, l.id AS access_line_id
FROM access_lines l
JOIN line_groups g ON g.id = l.line_group_id
WHERE g.enabled = TRUE
  AND l.line_group_id IS NOT NULL
"#;
