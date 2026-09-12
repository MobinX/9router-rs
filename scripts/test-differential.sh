#!/bin/sh
# Live differential: same logical request vs original 9Router and 9router-rs.
# Compares status + normalized shape; SSE compared as event-type sequences.
# Usage: ORIG=http://127.0.0.1:20128 RUST=http://127.0.0.1:22128 ./scripts/test-differential.sh
set -eu
node -e "
const orig = process.env.ORIG || 'http://127.0.0.1:20128';
const rust = process.env.RUST || 'http://127.0.0.1:22128';
const VOL = new Set(['id','created','timestamp','request_id','requestId','etag','syncedAt','session_id','nonce','state']);
const norm = (v) => JSON.parse(JSON.stringify(v, (k, x) => VOL.has(k) ? '<NORM>' : x));
const shape = (v) => Array.isArray(v) ? v.map(shape)
  : (v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, shape(v[k])])) : typeof v);
const elemShape = (j) => (j && Array.isArray(j.data) && j.data.length) ? shape(norm(j.data[0])) : null;
const sseTypes = (t) => t.split('\n\n').filter((b) => b.includes('data:')).map((b) => {
  const d = (b.match(/data:(.*)/s)||[])[1] || '';
  if (d.trim() === '[DONE]') return '[DONE]';
  const e = (b.match(/event:\s*(\S+)/)||[])[1] || 'message';
  return e;
});
async function one(base, m, p, b, stream) {
  const r = await fetch(base + p, {method: m, headers: {'content-type':'application/json'}, body: b ? JSON.stringify(b) : undefined});
  const rid = r.headers.get('x-request-id') ? 'yes' : 'no';
  if (stream) { const t = await r.text(); return {status: r.status, rid, types: sseTypes(t)}; }
  const j = await r.json().catch(() => ({}));
  const es = p === '/v1/models' ? elemShape(j) : null;
  return {status: r.status, rid, shape: JSON.stringify(es ?? shape(norm(j))).slice(0, 200)};
}
(async () => {
  const cases = [
    ['GET', '/api/health', null, false],
    ['GET', '/api/version', null, false],
    ['GET', '/v1/models', null, false],
    ['POST', '/v1/chat/completions', {model:'x',messages:[{role:'user',content:'hi'}]}, false],
    ['POST', '/v1/chat/completions', {model:'x',messages:[{role:'user',content:'hi'}],stream:true}, true],
    ['POST', '/v1/responses', {model:'x',input:'hi'}, false],
  ];
  let pass = 0, fail = 0;
  for (const [m, p, b, s] of cases) {
    let a, c;
    try { a = await one(orig, m, p, b, s); } catch (e) { console.log('SKIP(orig-down)', m, p); continue; }
    try { c = await one(rust, m, p, b, s); } catch (e) { console.log('FAIL(rust-down)', m, p); fail++; continue; }
    const sameStatus = a.status === c.status;
    const sameBody = JSON.stringify(a.shape ?? a.types) === JSON.stringify(c.shape ?? c.types);
    if (sameStatus && sameBody) { pass++; console.log('PASS', m, p, a.status); }
    else { fail++; console.log('DIFF', m, p, 'orig=' + JSON.stringify(a), 'rust=' + JSON.stringify(c)); }
  }
  console.log(pass + ' pass, ' + fail + ' fail');
  process.exit(fail ? 1 : 0);
})();
"