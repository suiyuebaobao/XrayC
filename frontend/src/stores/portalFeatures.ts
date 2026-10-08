// 入口显示策略来自网站设置，关闭入口不会代替后端权限/支付状态验证。
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { apiClient, type PortalFeatures } from '@/services/api';

export const defaultPortalFeatures = (): PortalFeatures => ({ plans: true, orders: true, redeem: true, invites: true, marketing: true });
export const usePortalFeaturesStore = defineStore('portal-features', () => {
  const features = ref<PortalFeatures>(defaultPortalFeatures());
  let loadedAt = 0;
  let pending: Promise<void> | undefined;
  function apply(value?: PortalFeatures) { features.value = value ?? defaultPortalFeatures(); loadedAt = Date.now(); }
  async function load() {
    if (pending) return pending;
    if (Date.now() - loadedAt < 60_000) return;
    pending = apiClient.getSalesLanding().then((value) => { apply(value.portalFeatures); })
      .catch(() => { /* 暂时读不到策略时保留已有入口，不误隐藏功能。 */ })
      .finally(() => { pending = undefined; });
    return pending;
  }
  return { features, apply, load };
});
