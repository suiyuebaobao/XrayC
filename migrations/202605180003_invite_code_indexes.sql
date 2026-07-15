-- 为用户自助邀请码列表和邀请来源追责补索引。
-- 邀请人和使用人都不能级联删除历史快照，索引只服务查询性能。

CREATE INDEX IF NOT EXISTS idx_invite_codes_inviter_user_id
    ON invite_codes(inviter_user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_invite_codes_used_by_user_id
    ON invite_codes(used_by_user_id, used_at DESC);
