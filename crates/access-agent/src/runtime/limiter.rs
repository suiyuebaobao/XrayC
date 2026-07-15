//! 本模块负责生成和应用用户级限速规则,按「每用户 fwmark」整形(不再按入口端口区分用户)。
//! 控制面已让每个限速用户的出站(到其出口)带上 `sockopt.mark=<该用户 mark>`,一个用户一个 mark、
//! 该用户全部 TCP+UDP 流量共用同一 mark。整形做在出口侧、按 fwmark 一把全限。
//!
//! 上行(用户上传=节点→出口,eth0 EGRESS,skb 已带 mark):物理网卡 eth0 root htb 上每用户一个上行
//! class、`rate=rate_limit_up_bps`(0→root),filter `handle <mark> fw flowid 1:<class>`,直接整形不经 IFB。
//!
//! 下行(用户下载=出口→节点,eth0 INGRESS):ingress qdisc + `matchall action connmark`(act_connmark
//! 从 conntrack 还原 mark 到 skb)+ `mirred egress redirect dev <ifb>`;IFB root htb 上每用户一个下行
//! class、`rate=rate_limit_down_bps`(0→root),filter `handle <mark> fw flowid 2:<class>`。
//!
//! CONNMARK save:iptables mangle OUTPUT 把出站 SO_MARK 存进 conntrack,供下行回程还原。
//! 内核 `act_connmark` 不可用时优雅降级:跳过整个下行 connmark/IFB 整形,保留上行 eth0 egress 按 mark
//! 整形,绝不 bail 整次 apply(详见 §7.7.1)。

use std::fs;

use anyhow::Context;
use tokio::process::Command;
use tracing::{info, warn};
use xrayc_xray_config::UserRateLimit;

use crate::config::AgentSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LimiterPlanSettings {
    pub(super) enabled: bool,
    pub(super) interface: String,
    pub(super) ifb_interface: String,
    pub(super) root_rate_bps: u64,
    /// 内核 `act_connmark` 是否可用:false 时优雅降级——跳过下行 connmark/IFB 整形、
    /// 保留上行 eth0 egress 按 mark 整形、绝不 bail 整次配置 apply。详见 §7.7.1。
    pub(super) connmark_available: bool,
}

pub(super) fn build_limiter_command_plan(
    settings: &LimiterPlanSettings,
    users: &[UserRateLimit],
) -> Vec<String> {
    if !settings.enabled {
        return cleanup_commands(settings);
    }

    let mut commands = Vec::new();
    commands.push(
        "# xrayc limiter: each user rate_limit_bps is shared by marked TCP and UDP traffic"
            .to_string(),
    );
    if !settings.connmark_available {
        // 内核 act_connmark 不可用:优雅降级——上行 eth0 egress 按 mark 整形仍生效,
        // 下行 connmark/IFB 整形(matchall redirect + IFB 下行类 + CONNMARK save/restore)整体跳过,
        // 绝不 bail 整次配置 apply。下载方向暂不受限速,上传仍严格按用户 fwmark 限速。详见 §7.7.1。
        commands.push(
            "# xrayc limiter degraded: act_connmark unavailable, download-direction connmark shaping skipped"
                .to_string(),
        );
    }
    commands.extend(iptables_prepare_commands());
    commands.extend(qdisc_cleanup_commands(settings));
    commands.extend(kernel_module_prepare_commands(settings.connmark_available));

    // 上行(用户上传=节点→出口)直接在物理网卡 eth0 egress 按 fwmark 整形,不经 IFB:
    // 出站到出口的 skb 已带该用户 sockopt.mark,root htb 上每用户一个上行 class + fw filter。
    commands.push(format!(
        "tc qdisc replace dev {} root handle 1: htb default 1",
        shell_word(&settings.interface)
    ));
    commands.push(format!(
        "tc class replace dev {} parent 1: classid 1:1 htb rate {} ceil {}",
        shell_word(&settings.interface),
        format_tc_rate(settings.root_rate_bps),
        format_tc_rate(settings.root_rate_bps)
    ));

    // 下行(用户下载=出口→节点)经 eth0 ingress + act_connmark 还原 mark 后 redirect 进 IFB 整形。
    // 这一整组(IFB 接口、ingress qdisc、matchall+connmark redirect、IFB root htb)都依赖 act_connmark,
    // connmark 不可用时整体跳过——优雅降级:下载暂不限速、上行仍严格按 mark 限速、绝不 bail。
    if settings.connmark_available {
        commands.push(format!(
            "ip link show {} >/dev/null 2>&1 || ip link add {} type ifb",
            shell_word(&settings.ifb_interface),
            shell_word(&settings.ifb_interface)
        ));
        commands.push(format!(
            "ip link set {} up",
            shell_word(&settings.ifb_interface)
        ));
        commands.push(format!(
            "tc qdisc replace dev {} ingress",
            shell_word(&settings.interface)
        ));
        // matchall+connmark:act_connmark 从 conntrack 还原 per-user mark 到 skb,
        // 再把回程整体 redirect 进 IFB,由 IFB 上 `fw` 过滤器按 mark 分流到各自下行 class。
        commands.push(format!(
            "tc filter add dev {} parent ffff: protocol ip prio 32766 matchall action connmark action mirred egress redirect dev {}",
            shell_word(&settings.interface),
            shell_word(&settings.ifb_interface)
        ));
        commands.push(format!(
            "tc qdisc replace dev {} root handle 2: htb default 1",
            shell_word(&settings.ifb_interface)
        ));
        commands.push(format!(
            "tc class replace dev {} parent 2: classid 2:1 htb rate {} ceil {}",
            shell_word(&settings.ifb_interface),
            format_tc_rate(settings.root_rate_bps),
            format_tc_rate(settings.root_rate_bps)
        ));
    }

    let mut users = users
        .iter()
        .filter(|user| user.rate_limit_bps > 0 && user.mark > 0 && user.class_id > 1)
        .collect::<Vec<_>>();
    users.sort_by_key(|user| (user.class_id, user.mark));
    for user in users {
        // 上行(eth0 egress 类 1:N)用上行速率。某方向速率为 0 表示不限该方向,退回根速率(放行,不卡该方向)。
        let up_rate = format_tc_rate(directional_or_root(
            user.rate_limit_up_bps,
            settings.root_rate_bps,
        ));
        commands.push(format!(
            "tc class replace dev {} parent 1: classid 1:{} htb rate {} ceil {}",
            shell_word(&settings.interface),
            user.class_id,
            up_rate,
            up_rate
        ));
        // 一个用户一条 fw filter:该 mark catch 该用户全部 TCP+UDP 流量,绝不按协议拆。
        commands.push(format!(
            "tc filter replace dev {} parent 1: protocol ip prio {} handle {} fw flowid 1:{}",
            shell_word(&settings.interface),
            user.class_id,
            user.mark,
            user.class_id
        ));
        // 下行(IFB 类 2:N)用下行速率 + 出站 CONNMARK save:都属下行 connmark 整形链路,
        // connmark 不可用时整体跳过(还原侧无消费者,建了也喂不进流量)。
        if settings.connmark_available {
            let down_rate = format_tc_rate(directional_or_root(
                user.rate_limit_down_bps,
                settings.root_rate_bps,
            ));
            commands.push(format!(
                "tc class replace dev {} parent 2: classid 2:{} htb rate {} ceil {}",
                shell_word(&settings.ifb_interface),
                user.class_id,
                down_rate,
                down_rate
            ));
            commands.push(format!(
                "tc filter replace dev {} parent 2: protocol ip prio {} handle {} fw flowid 2:{}",
                shell_word(&settings.ifb_interface),
                user.class_id,
                user.mark,
                user.class_id
            ));
            // CONNMARK --save-mark 把出站 SO_MARK 存进 conntrack,供回程 act_connmark 还原。
            commands.push(format!(
                "iptables -t mangle -A XRAYC_LIMITER_OUTPUT -m mark --mark {} -j CONNMARK --save-mark",
                user.mark
            ));
        }
    }
    // CONNMARK --restore-mark 把 conntrack 标记取回 skb(netfilter 侧),与 tc 层 act_connmark 互补。
    // connmark 不可用时跳过:无下行整形,restore 也没有下游消费者。
    if settings.connmark_available {
        commands.push(
            "iptables -t mangle -A XRAYC_LIMITER_PREROUTING -j CONNMARK --restore-mark".to_string(),
        );
    }
    commands
}

pub(super) async fn reconcile_limiter(
    settings: &AgentSettings,
    users: &[UserRateLimit],
    connmark_available: bool,
) -> anyhow::Result<()> {
    let plan_settings = LimiterPlanSettings {
        enabled: settings.rate_limiter_enabled,
        interface: settings.rate_limiter_interface.clone(),
        ifb_interface: settings.rate_limiter_ifb_interface.clone(),
        root_rate_bps: settings.rate_limiter_root_rate_bps,
        connmark_available,
    };
    let commands = build_limiter_command_plan(&plan_settings, users);
    persist_limiter_plan(settings, &commands)?;
    if settings.rate_limiter_dry_run {
        return Ok(());
    }
    if !settings.rate_limiter_enabled {
        return Ok(());
    }
    for command in commands {
        run_limiter_command(&command).await?;
    }
    Ok(())
}

fn persist_limiter_plan(settings: &AgentSettings, commands: &[String]) -> anyhow::Result<()> {
    if let Some(parent) = settings.rate_limiter_plan_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
    }
    let mut content = "#!/bin/sh\nset -eu\n".to_string();
    for command in commands {
        content.push_str(command);
        content.push('\n');
    }
    fs::write(&settings.rate_limiter_plan_path, content)
        .with_context(|| format!("write {:?}", settings.rate_limiter_plan_path))?;
    Ok(())
}

async fn run_limiter_command(command: &str) -> anyhow::Result<()> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .with_context(|| format!("run limiter command: {command}"))?;
    if output.status.success() {
        return Ok(());
    }
    anyhow::bail!("limiter command failed: {command}");
}

fn iptables_prepare_commands() -> Vec<String> {
    vec![
        "iptables -t mangle -N XRAYC_LIMITER_OUTPUT 2>/dev/null || true".to_string(),
        "iptables -t mangle -N XRAYC_LIMITER_PREROUTING 2>/dev/null || true".to_string(),
        "iptables -t mangle -F XRAYC_LIMITER_OUTPUT".to_string(),
        "iptables -t mangle -F XRAYC_LIMITER_PREROUTING".to_string(),
        "iptables -t mangle -C OUTPUT -j XRAYC_LIMITER_OUTPUT 2>/dev/null || iptables -t mangle -A OUTPUT -j XRAYC_LIMITER_OUTPUT".to_string(),
        "iptables -t mangle -C PREROUTING -j XRAYC_LIMITER_PREROUTING 2>/dev/null || iptables -t mangle -A PREROUTING -j XRAYC_LIMITER_PREROUTING".to_string(),
    ]
}

fn kernel_module_prepare_commands(connmark_available: bool) -> Vec<String> {
    let mut commands = vec![
        "modprobe sch_htb || true".to_string(),
        "modprobe cls_fw || true".to_string(),
        "modprobe cls_u32 || true".to_string(),
    ];
    // act_connmark：tc 层把 conntrack 标记读回 skb，下行回程整形必需。
    // 已自检判定不可用时跳过这条 modprobe（旧实现这里加载失败会拖垮整次 apply）。
    if connmark_available {
        commands.push("modprobe act_connmark || true".to_string());
    }
    commands.extend([
        // act_mirred/ifb 是下行 redirect 进 IFB 要用的模块（仅 connmark 可用路径才走到）。
        "modprobe act_mirred || true".to_string(),
        "modprobe ifb numifbs=1 || true".to_string(),
    ]);
    commands
}

fn cleanup_commands(settings: &LimiterPlanSettings) -> Vec<String> {
    let mut commands = qdisc_cleanup_commands(settings);
    commands.extend([
        "iptables -t mangle -F XRAYC_LIMITER_OUTPUT 2>/dev/null || true".to_string(),
        "iptables -t mangle -F XRAYC_LIMITER_PREROUTING 2>/dev/null || true".to_string(),
    ]);
    commands
}

fn qdisc_cleanup_commands(settings: &LimiterPlanSettings) -> Vec<String> {
    vec![
        format!(
            "tc qdisc del dev {} root 2>/dev/null || true",
            shell_word(&settings.interface)
        ),
        format!(
            "tc qdisc del dev {} ingress 2>/dev/null || true",
            shell_word(&settings.interface)
        ),
        format!(
            "tc qdisc del dev {} root 2>/dev/null || true",
            shell_word(&settings.ifb_interface)
        ),
    ]
}

/// 方向速率取值:该方向有正值则用之;为 0(表示不限该方向)则退回根速率(放行,不卡该方向)。
fn directional_or_root(direction_bps: u64, root_rate_bps: u64) -> u64 {
    if direction_bps > 0 {
        direction_bps
    } else {
        root_rate_bps
    }
}

fn format_tc_rate(rate_bps: u64) -> String {
    if rate_bps >= 1_000_000 && rate_bps.is_multiple_of(1_000_000) {
        return format!("{}mbit", rate_bps / 1_000_000);
    }
    if rate_bps >= 1_000 && rate_bps.is_multiple_of(1_000) {
        return format!("{}kbit", rate_bps / 1_000);
    }
    format!("{rate_bps}bit")
}

fn shell_word(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
        .collect::<String>()
}

/// 判 `tc qdisc show dev X` 输出里是否存在指定 handle 的 htb 根 qdisc(整形存在的标志)。
/// 上行整形根为 `htb 1:`、下行(IFB)为 `htb 2:`;被 `tc qdisc del dev X root` 拆掉后回落
/// 默认 qdisc(noqueue/mq/pfifo_fast 等),输出不含该串。handle 后紧跟冒号做精确匹配,
/// 避免 `htb 11:` 被误判成 `htb 1:`。纯函数便于单测。
pub(super) fn shaping_present_in_qdisc_output(output: &str, handle: &str) -> bool {
    output.contains(&format!("htb {handle}:"))
}

/// 探测当前限速整形是否完好(读操作,不改任何东西):上行物理网卡 root 应为 htb 1:;
/// connmark 可用时下行 IFB root 应为 htb 2:。探测命令执行失败一律按"不完好"处理
/// (触发幂等重建,reconcile 幂等、重建无副作用,宁可多建一次也不放任限速失效)。
async fn shaping_intact(settings: &AgentSettings, connmark_available: bool) -> bool {
    if !qdisc_has_htb_handle(&settings.rate_limiter_interface, "1").await {
        return false;
    }
    // 下行整形仅在 connmark 可用路径存在;降级态下本就无下行整形,不作为"缺失"判据。
    if connmark_available && !qdisc_has_htb_handle(&settings.rate_limiter_ifb_interface, "2").await
    {
        return false;
    }
    true
}

/// 跑 `tc qdisc show dev <interface>` 读回输出,判是否含指定 htb handle 根。只读、无副作用。
async fn qdisc_has_htb_handle(interface: &str, handle: &str) -> bool {
    let output = Command::new("tc")
        .args(["qdisc", "show", "dev", interface])
        .output()
        .await;
    match output {
        Ok(out) => shaping_present_in_qdisc_output(&String::from_utf8_lossy(&out.stdout), handle),
        // 探测本身失败(命令缺失/网卡不存在等)按"不完好"→交给 reconcile 幂等重建兜底。
        Err(_) => false,
    }
}

/// 心跳级限速整形自愈:整形被外部拆掉(降级恢复/网卡变动/手动清/某些重启后配置未变、
/// 中心不再下发 config)时,不等配置变化就在下个心跳幂等重建,杜绝限速静默失效。
/// 整形完好则只探测、不动整形(不干扰在途流量、不引入抖动);缺失才 reconcile
/// (幂等;失败沿用 P1-2 降级只 warn、不 bail,下个心跳再试)。
pub(super) async fn ensure_limiter_shaping(
    settings: &AgentSettings,
    users: &[UserRateLimit],
    connmark_available: bool,
) {
    // 未开限速器 / 本节点无限速用户:本就不应有整形,不自愈、不误建。
    if !settings.rate_limiter_enabled || users.is_empty() {
        return;
    }
    if shaping_intact(settings, connmark_available).await {
        return;
    }
    match reconcile_limiter(settings, users, connmark_available).await {
        Ok(()) => info!("检测到限速整形缺失,已自愈幂等重建"),
        Err(error) => {
            warn!(error = %error, "限速整形自愈重建失败(已降级,下个心跳再试)")
        }
    }
}
