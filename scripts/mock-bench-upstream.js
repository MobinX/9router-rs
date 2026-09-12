// Fixed fast mock upstream for benchmarks (also serves gateway-shaped paths).
const http = require('http');
const CHAT = JSON.stringify({id:'chatcmpl-bench',object:'chat.completion',created:1,model:'bench',
  choices:[{index:0,message:{role:'assistant',content:'hello benchmark world'},finish_reason:'stop'}],
  usage:{prompt_tokens:3,completion_tokens:3,total_tokens:6}});
const SSE = 'data: {"id":"chatcmpl-1","choices":[{"delta":{"content":"hello benchmark world"}}]}\n\ndata: [DONE]\n\n';
http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => body += c);
  req.on('end', () => {
    const stream = body.includes('"stream":true');
    if (stream) { res.writeHead(200, {'content-type':'text/event-stream'}); res.end(SSE); }
    else { res.writeHead(200, {'content-type':'application/json'}); res.end(CHAT); }
  });
}).listen(23128, () => console.log('mock on 23128'));
