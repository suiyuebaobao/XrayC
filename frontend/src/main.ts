// 本文件是前端应用启动入口。
// 它负责创建 Vue 应用、挂载 Pinia、注册路由和 Element Plus。
// 全局样式也在这里导入，保证浏览器加载时只初始化一次。
// 页面业务逻辑不得写在此文件中。
import { createPinia } from 'pinia';
import { createApp } from 'vue';
import ElementPlus from 'element-plus';
import 'element-plus/dist/index.css';
import App from './App.vue';
import { router } from './router';
import './styles/main.css';
import './styles/shell.css';

createApp(App).use(createPinia()).use(router).use(ElementPlus).mount('#app');
