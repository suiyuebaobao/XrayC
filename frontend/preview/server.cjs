// 独立本机 UI 演示服务：仅监听回环地址，不连接正式站或数据库。
const http = require('node:http');
const { randomUUID } = require('node:crypto');
const now = new Date().toISOString();
const nodes = [
 {id:'node-sg',name:'新加坡 · 接入一',public_host:'198.51.100.10',ip_direct_address:'198.51.100.10',public_port:443,status:'healthy',health_status:'healthy',config_synced:true,config_dirty:false,agent_version:'XrayC Agent',last_heartbeat_at:now},
 {id:'node-jp',name:'东京 · 接入二',public_host:'198.51.100.20',ip_direct_address:'198.51.100.20',public_port:443,status:'healthy',health_status:'healthy',config_synced:true,config_dirty:false,agent_version:'XrayC Agent',last_heartbeat_at:now},
 {id:'node-us',name:'洛杉矶 · 接入三',public_host:'198.51.100.30',ip_direct_address:'198.51.100.30',public_port:443,status:'healthy',health_status:'degraded',config_synced:false,config_dirty:true,agent_version:'XrayC Agent',last_heartbeat_at:now},
];
const entries = nodes.map((n,i)=>({id:'entry-'+i,access_node_id:n.id,access_node_name:n.name,name:['新加坡 REALITY','东京 REALITY','美国 XHTTP'][i],listen_host:n.public_host,listen_port:443,protocol:'vless',transport:i===2?'xhttp':'tcp',security:i===2?'tls':'reality',server_name:'example.com',enabled:true,sort_weight:100}));
const exits=[{id:'exit-hk',exit_resource_id:'resource-hk',resource_name:'香港自建出口',name:'香港优选出口',host:'203.0.113.10',port:443,outbound_type:'vless',enabled:true,exit_resource_enabled:true,status:'healthy',healthy:true,last_probe_status:'healthy',last_probe_at:now},{id:'exit-jp',exit_resource_id:'resource-jp',resource_name:'日本自建出口',name:'日本原生出口',host:'203.0.113.20',port:443,outbound_type:'vless',enabled:true,exit_resource_enabled:true,status:'healthy',healthy:true,last_probe_status:'healthy',last_probe_at:now}];
let bindings=[['line-1',0,0,'新加坡 · 日常优选',true],['line-2',1,1,'日本 · 流媒体专线',true],['line-3',0,1,'新加坡 → 日本 · 备用',true],['line-4',2,0,'美国 · 工作专线',true],['line-5',1,0,'香港 · 备用线路',false]].map(([id,i,j,name,enabled])=>({id,access_entry_id:entries[i].id,access_entry_name:entries[i].name,access_node_id:nodes[i].id,access_node_name:nodes[i].name,exit_endpoint_id:exits[j].id,exit_endpoint_name:exits[j].name,name,enabled,sort_weight:100,remark:'示例线路，不连接真实服务器'}));
const groups=[{id:'group-daily',name:'日常线路',enabled:true,binding_node_ids:['line-1','line-3','line-4','line-5']},{id:'group-stream',name:'流媒体',enabled:true,binding_node_ids:['line-2']}];
let settings={profile_name:'XrayC',mixed_port:7890,allow_lan:false,mode:'rule',log_level:'info',update_interval_hours:24,default_rules:['GEOIP,CN,DIRECT'],auto_test_enabled:false,auto_test_name:'自动选择',auto_test_url:'https://www.gstatic.com/generate_204',auto_test_interval_seconds:86400,block_unhealthy_lines:false};
const events=[];
const server=http.createServer(async(req,res)=>{
 const u=new URL(req.url,'http://localhost');const path=u.pathname;let body='';for await(const c of req)body+=c;let data={};try{if(body)data=JSON.parse(body)}catch{res.writeHead(400);res.end('{}');return}
 res.setHeader('Content-Type','application/json; charset=utf-8');let result={};let status=200;
 if(path==='/__events'){res.end(JSON.stringify(events));return;}
 if(req.method!=='GET') {
  events.push({method:req.method,path,body:data});
  const supported = ['/api/auth/login','/api/auth/logout','/api/auth/refresh','/api/admin/subscription-settings'].includes(path) || /\/api\/admin\/access-entries\/[^/]+\/exit-bindings/.test(path);
  if (!supported) { res.writeHead(501); res.end(JSON.stringify({success:false,message:'本机预览暂未模拟此操作，正式数据不会改变'})); return; }
 }
 if(path==='/api/auth/security')result={captcha_enabled:false,login_captcha_enabled:false,admin_login_captcha_enabled:false};
 else if(path==='/api/auth/login'||path==='/api/auth/refresh')result={access_token:'local-preview-only',user:{id:'user-admin',account:'admin',name:'管理员',role:'admin',is_admin:true}};
 else if(path==='/api/auth/logout')result={};
 else if(path==='/api/admin/access-routing')result={access_nodes:nodes,access_lines:[],exit_pools:[],line_groups:groups};
 else if(path==='/api/admin/access-entries')result={access_entries:entries};
 else if(path==='/api/admin/access-entry-exit-bindings')result={access_entry_exit_bindings:bindings};
 else if(/\/api\/admin\/access-entries\/[^/]+\/exit-bindings/.test(path)&&req.method==='POST'){
  const eid=path.split('/')[4],entry=entries.find(x=>x.id===eid),exit=exits.find(x=>x.id===data.exit_endpoint_id);if(!entry||!exit){status=422;result={message:'请选择入口与出口'}}else{const row={id:randomUUID(),access_entry_id:eid,access_entry_name:entry.name,access_node_id:entry.access_node_id,access_node_name:entry.access_node_name,exit_endpoint_id:exit.id,exit_endpoint_name:exit.name,name:data.name,enabled:true,sort_weight:100};bindings.push(row);result={id:row.id}}}
 else if(path==='/api/admin/access-nodes')result=nodes;
 else if(path==='/api/admin/exit-endpoints')result={exit_endpoints:exits,items:exits};
 else if(path==='/api/admin/exit-resources')result={exit_resources:[{id:'resource-hk',name:'香港自建出口',enabled:true,ownership:'self_hosted',access_node_id:'node-sg'},{id:'resource-jp',name:'日本自建出口',enabled:true,ownership:'self_hosted',access_node_id:'node-jp'}]};
 else if(path==='/api/admin/exit-pools')result=[];
 else if(path==='/api/admin/deployment-tasks')result={items:[]};
 else if(path==='/api/admin/access-operations/summary')result={active_users:24,access_line_count:bindings.filter(x=>x.enabled).length,active_access_lines:4,healthy_exit_pools:2,access_node_count:3,config_dirty_nodes:1,recent_events:[{id:1,level:'warning',title:'洛杉矶接入节点等待应用配置',time:'等待节点回执'},{id:2,level:'info',title:'日本流媒体线路配置已同步',time:'示例事件'},{id:3,level:'info',title:'数据库备份完成',time:'示例事件'}]};
 else if(path==='/api/admin/subscription-settings'){if(req.method==='PUT')settings={...settings,...data};result=settings;}
 else if(path==='/api/admin/subscription-rule-sets')result={items:[{id:'rule-stream',name:'流媒体规则',enabled:true,rules:['DOMAIN-SUFFIX,example.test,PROXY'],description:'示例规则库',binding_count:1}]};
 else if(path==='/api/admin/plans')result={plans:[{id:'plan-1',name:'一年无流量限制',enabled:true,traffic_limit_bytes:-1,rate_limit_bps:0,rate_limit_up_bps:0,rate_limit_down_bps:0,duration_days:365,currency:'CNY',price_cents:0,billing_multiplier:1,line_groups:[{line_group_id:'group-daily',billing_multiplier:1},{line_group_id:'group-stream',billing_multiplier:1}]}]};
 else if(path==='/api/admin/users')result={users:[{id:'user-1',email:'demo@example.test',name:'示例用户',status:'active',plan_name:'一年无流量限制',subscription_active:true,expires_at:'2027-10-05T00:00:00Z',traffic_gb:{used_gb:26.5,total_gb:-1},remaining_gb:-1,is_admin:false}]};
 else if(path==='/api/admin/orders')result={orders:[]};
 else if(path==='/api/admin/audit-logs')result={items:[]};
 else if(path==='/api/admin/access-operations/ledger-ranking')result={items:[]};
 else if(path==='/api/admin/monitor/node-traffic')result={items:[],summary:{}};
 else if(path.endsWith('/platform-metrics'))result={cpu:{},memory:{},disk:{},database:{tables:[]},generated_at:now};
 else if(path==='/api/sales-landing')result={hero:{title:'XrayC 本机界面预览',subtitle:'示例数据，所有操作与正式站隔离。'}};
 else if(path==='/api/payment/channels')result={enabled:false,channels:[]};
 else if(path==='/api/admin/invite-codes'||path==='/api/admin/redeem-codes')result={items:[]};
 else if(path.includes('/local-exit-lines'))result={items:[]};
 else if(!path.startsWith('/api/')){status=404;result={message:'Preview endpoint not found'}}
 res.writeHead(status);res.end(JSON.stringify({success:status===200,data:result}));
});server.listen(18880,'127.0.0.1',()=>console.log('Isolated preview API listening on 127.0.0.1:18880'));
