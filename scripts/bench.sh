#!/bin/sh
# Bench with keepalive. Usage: TARGET=http://127.0.0.1:22128 N=200 C=20 sh scripts/bench.sh
set -eu
node -e "
const http = require('http');
const T = process.env.TARGET || 'http://127.0.0.1:22128';
const N = parseInt(process.env.N || '200', 10);
const C = parseInt(process.env.C || '20', 10);
const agent = new http.Agent({keepAlive: true, maxSockets: 100});
const URL = T + '/v1/chat/completions';
function once(stream) {
  return new Promise((resolve, reject) => {
    const t0 = process.hrtime.bigint();
    const body = JSON.stringify(stream
      ? {model:'bench',messages:[{role:'user',content:'hi'}],stream:true}
      : {model:'bench',messages:[{role:'user',content:'hi'}]});
    const r = http.request(URL, {method:'POST',agent,headers:{'content-type':'application/json','content-length':Buffer.byteLength(body)}}, (resp) => {
      resp.resume(); resp.on('end', () => resolve(Number(process.hrtime.bigint()-t0)/1e6)); resp.on('error', reject);
    });
    r.on('error', reject); r.end(body);
  });
}
(async () => {
  for (let i = 0; i < 10; i++) await once(false);
  let t0 = process.hrtime.bigint();
  for (let i = 0; i < 20; i++) await once(false);
  const seq = Number(process.hrtime.bigint()-t0)/1e6/20;
  t0 = process.hrtime.bigint();
  for (let i = 0; i < N; i += C) await Promise.all(Array.from({length:C}, () => once(false)));
  const conc = Number(process.hrtime.bigint()-t0)/1e6;
  t0 = process.hrtime.bigint();
  for (let i = 0; i < 20; i++) await once(true);
  const sse = Number(process.hrtime.bigint()-t0)/1e6/20;
  console.log(JSON.stringify({target:T, seq_ms:+seq.toFixed(2), conc_req_s:+(N/(conc/1000)).toFixed(1), sse_ms:+sse.toFixed(2)}));
  process.exit(0);
})();
"