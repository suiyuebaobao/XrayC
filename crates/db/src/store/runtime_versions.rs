//! 记录控制面实例与节点版本。缺少回报明确保留未知，不猜测镜像或内核版本。
use crate::*;
use serde_json::{json, Value};
use uuid::Uuid;

impl PgStore {
    pub async fn register_worker_version(
        &self,
        package: &str,
        release: &str,
        interval: u64,
    ) -> Result<Uuid, DbError> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO service_runtime_versions (instance_id, service_name, package_version, release_id, heartbeat_interval_seconds)
            VALUES ($1, 'worker', $2, $3, $4)")
            .bind(id).bind(package).bind(release).bind(interval.clamp(1, 86400) as i32)
            .execute(&self.pool).await?;
        Ok(id)
    }

    pub async fn touch_worker_version(&self, instance: Uuid) -> Result<(), DbError> {
        sqlx::query("UPDATE service_runtime_versions SET heartbeat_at=now() WHERE instance_id=$1")
            .bind(instance)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn record_agent_versions(
        &self,
        node: Uuid,
        agent: Option<&str>,
        xray: Option<&str>,
    ) -> Result<(), DbError> {
        let clean = |value: Option<&str>| {
            value
                .map(|value| value.trim().chars().take(160).collect::<String>())
                .filter(|value| !value.is_empty())
        };
        let agent = clean(agent);
        let xray = clean(xray);
        if agent.is_none() && xray.is_none() {
            return Ok(());
        }
        sqlx::query("UPDATE access_nodes SET agent_version=COALESCE($2,agent_version),
            xray_version=CASE WHEN $3::text IS NOT NULL THEN $3 WHEN $2::text IS NOT NULL AND agent_version<>$2 THEN '' ELSE xray_version END
            WHERE id=$1 AND (($2::text IS NOT NULL AND agent_version IS DISTINCT FROM $2)
                OR ($3::text IS NOT NULL AND xray_version IS DISTINCT FROM $3))")
            .bind(node).bind(agent).bind(xray).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn runtime_versions_json(&self) -> Result<Value, DbError> {
        let workers = sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object(
            'instance_id',instance_id,'package_version',package_version,'release_id',release_id,
            'started_at',started_at,'heartbeat_at',heartbeat_at,
            'fresh',heartbeat_at > now()-make_interval(secs=>GREATEST(90,heartbeat_interval_seconds*3)))
            FROM service_runtime_versions WHERE service_name='worker' ORDER BY heartbeat_at DESC LIMIT 30")
            .fetch_all(&self.pool).await?;
        let nodes = sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('id',id,'name',name,
            'agent_version',agent_version,'xray_version',xray_version,'last_heartbeat_at',last_heartbeat_at)
            FROM access_nodes ORDER BY name,id").fetch_all(&self.pool).await?;
        Ok(json!({"workers":workers,"nodes":nodes}))
    }
}
