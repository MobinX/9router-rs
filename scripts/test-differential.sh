#!/bin/sh
# Differential: same logical request vs original 9Router and 9router-rs, normalized diff.
# Usage: GATEWAY_URL=http://127.0.0.1:20128 ./scripts/test-differential.sh
set -eu
node -e "
const url = process.env.GATEWAY_URL || 'http://127.0.0.1:20128';
(async () => {
  const norm = (v) => JSON.parse(JSON.stringify(v, (k, x) =>
    ['id','created','timestamp','request_id','etag','syncedAt'].includes(k) ? '<NORM>' : x));
  const cases = [
    ['GET', '/api/health', null],
    ['GET', '/api/version', null],
    ['POST', '/v1/chat/completions', {model:'gpt-4o',messages:[{role:'user',content:'hi'}]}],
  ];
  for (const [m, p, b] of cases) {
    const r = await fetch(url + p, {method: m, headers: {'content-type':'application/json','authorization':'Bearer diff'}, body: b ? JSON.stringify(b) : undefined});
    const j = await r.json().catch(() => ({}));
    console.log(r.status, p, JSON.stringify(norm(j)).slice(0, 160));
  }
})();
"
