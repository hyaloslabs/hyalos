// Spike #72: a scripted stand-in for the Hyalos broker. It has no model.
// It listens on a unix socket and speaks just enough Anthropic Messages and
// OpenAI Responses (streaming and not) to drive a harness through one tool
// call that runs `hyalos-submit`, then a final text answer. It logs everything
// the harness sends so we can see exactly what crosses the channel.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';

const dir = process.argv[2];
const sock = process.argv[3] || path.join(dir, 'broker.sock');
const logFile = path.join(dir, 'broker.log');
const bodyDir = path.join(dir, 'bodies');
fs.mkdirSync(bodyDir, { recursive: true });
try { fs.unlinkSync(sock); } catch {}
let n = 0;
const submissions = [];

const log = (o) => fs.appendFileSync(logFile, JSON.stringify({ t: new Date().toISOString(), ...o }) + '\n');
const sse = (res, events) => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  for (const [ev, data] of events) res.write(`event: ${ev}\ndata: ${JSON.stringify(data)}\n\n`);
  res.end();
};
const json = (res, code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };

const SUBMIT = `hyalos-submit '{"summary":"spike72 ok"}'`;

function anthropic(body, res) {
  const tools = (body.tools || []).map((t) => t.name);
  const msgs = body.messages || [];
  const hasResult = msgs.some((m) => Array.isArray(m?.content) && m.content.some((c) => c.type === 'tool_result'));
  const bash = tools.includes('Bash');
  const model = body.model || 'stub';
  const wantsTool = bash && !hasResult && !body.system?.toString().includes('Extract the command prefix');
  const usage = { input_tokens: 10, output_tokens: 5 };
  const head = { type: 'message_start', message: { id: 'msg_stub', type: 'message', role: 'assistant', model, content: [], stop_reason: null, stop_sequence: null, usage } };
  if (wantsTool) {
    const input = { command: SUBMIT, description: 'submit result' };
    if (!body.stream) return json(res, 200, { id: 'msg_stub', type: 'message', role: 'assistant', model, content: [{ type: 'tool_use', id: 'toolu_stub1', name: 'Bash', input }], stop_reason: 'tool_use', stop_sequence: null, usage });
    return sse(res, [
      ['message_start', head],
      ['content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id: 'toolu_stub1', name: 'Bash', input: {} } }],
      ['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }],
      ['content_block_stop', { type: 'content_block_stop', index: 0 }],
      ['message_delta', { type: 'message_delta', delta: { stop_reason: 'tool_use', stop_sequence: null }, usage: { output_tokens: 20 } }],
      ['message_stop', { type: 'message_stop' }],
    ]);
  }
  const text = hasResult ? 'Result submitted.' : 'ok';
  if (!body.stream) return json(res, 200, { id: 'msg_stub', type: 'message', role: 'assistant', model, content: [{ type: 'text', text }], stop_reason: 'end_turn', stop_sequence: null, usage });
  return sse(res, [
    ['message_start', head],
    ['content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }],
    ['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }],
    ['content_block_stop', { type: 'content_block_stop', index: 0 }],
    ['message_delta', { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 5 } }],
    ['message_stop', { type: 'message_stop' }],
  ]);
}

function responses(body, res) {
  const fnTools = (body.tools || []).filter((t) => t.type === 'function').map((t) => t.name);
  const input = Array.isArray(body.input) ? body.input : [];
  const hasResult = input.some((i) => i.type === 'function_call_output' || i.type === 'custom_tool_call_output');
  const pick = ['shell_command', 'shell', 'exec_command'].find((x) => fnTools.includes(x));
  const model = body.model || 'stub';
  const rid = 'resp_stub' + n;
  const base = { id: rid, object: 'response', model, status: 'in_progress', output: [] };
  const done = (output) => ({ ...base, status: 'completed', output, usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15 } });
  if (pick && !hasResult) {
    const args = pick === 'shell' ? { command: ['bash', '-lc', SUBMIT] } : pick === 'shell_command' ? { command: SUBMIT } : { cmd: SUBMIT };
    const item = { type: 'function_call', id: 'fc_stub', call_id: 'call_stub1', name: pick, arguments: JSON.stringify(args), status: 'completed' };
    return sse(res, [
      ['response.created', { type: 'response.created', response: base }],
      ['response.output_item.added', { type: 'response.output_item.added', output_index: 0, item: { ...item, arguments: '', status: 'in_progress' } }],
      ['response.output_item.done', { type: 'response.output_item.done', output_index: 0, item }],
      ['response.completed', { type: 'response.completed', response: done([item]) }],
    ]);
  }
  const msg = { type: 'message', id: 'msg_stub', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: hasResult ? 'Result submitted.' : 'ok', annotations: [] }] };
  return sse(res, [
    ['response.created', { type: 'response.created', response: base }],
    ['response.output_item.added', { type: 'response.output_item.added', output_index: 0, item: { ...msg, content: [] } }],
    ['response.output_item.done', { type: 'response.output_item.done', output_index: 0, item: msg }],
    ['response.completed', { type: 'response.completed', response: done([msg]) }],
  ]);
}

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const raw = Buffer.concat(chunks).toString('utf8');
    const id = ++n;
    let body = null;
    try { body = raw ? JSON.parse(raw) : null; } catch {}
    const h = req.headers;
    const hdr = { host: h.host, 'user-agent': h['user-agent'], 'x-api-key': h['x-api-key'], authorization: h.authorization, 'anthropic-version': h['anthropic-version'], 'anthropic-beta': h['anthropic-beta'], originator: h.originator, 'content-type': h['content-type'] };
    log({ id, method: req.method, url: req.url, hdr, model: body?.model, stream: body?.stream, tools: (body?.tools || []).map((t) => t.name || t.type), bytes: raw.length });
    if (raw) fs.writeFileSync(path.join(bodyDir, `${String(id).padStart(3, '0')}.json`), raw);
    // Plain-HTTP proxy request (absolute URI): allowlist by host.
    if (/^https?:\/\//i.test(req.url)) {
      const host = new URL(req.url).hostname; const ok = allowed(host);
      log({ id, proxyHttp: host, allowed: ok });
      if (!ok) return json(res, 403, { error: 'egress denied: ' + host });
      return fetch(req.url, { method: req.method }).then(async (r) => { res.writeHead(r.status, { 'content-type': r.headers.get('content-type') || 'text/plain' }); res.end(Buffer.from(await r.arrayBuffer())); }).catch((e) => json(res, 502, { error: String(e) }));
    }
    const u = req.url.split('?')[0];
    // Fault injection for model endpoints, driven by run/fault.json: {status, count, retryAfter, type}
    const fault = takeFault(u);
    if (fault) {
      log({ id, fault: fault.status, left: fault.count });
      const isA = /messages$/.test(u);
      const eb = isA ? { type: 'error', error: { type: fault.type || 'rate_limit_error', message: fault.message || 'injected fault' } }
                     : { error: { message: fault.message || 'injected fault', type: fault.type || 'rate_limit_exceeded', code: fault.code || fault.type || 'rate_limit_exceeded' } };
      res.writeHead(fault.status, { 'content-type': 'application/json', ...(fault.retryAfter ? { 'retry-after': String(fault.retryAfter) } : {}) });
      return res.end(JSON.stringify(eb));
    }
    if (req.method === 'POST' && /\/v1\/messages$/.test(u) && body && process.env.PASSTHROUGH === 'both') {
      // Real-broker behavior for Anthropic Messages: swap the credential, drop
      // provider-hosted tools and hosted MCP, forward only the headers the API needs.
      const isHosted = (t) => /^(web_search|web_fetch|code_execution|mcp)/.test(t.type || '');
      const before = (body.tools || []).map((t) => t.type || 'custom');
      body.tools = (body.tools || []).filter((t) => !isHosted(t));
      const hadMcp = !!body.mcp_servers; delete body.mcp_servers;
      const stripped = (body.tools ? before : []).filter((t) => /^(web_search|web_fetch|code_execution|mcp)/.test(t));
      const fh = { 'content-type': 'application/json', 'x-api-key': process.env.ANTHROPIC_API_KEY, 'anthropic-version': req.headers['anthropic-version'] || '2023-06-01' };
      if (req.headers['anthropic-beta']) fh['anthropic-beta'] = req.headers['anthropic-beta'];
      (async () => {
        const up = await fetch('https://api.anthropic.com' + req.url, { method: 'POST', headers: fh, body: JSON.stringify(body) });
        res.writeHead(up.status, { 'content-type': up.headers.get('content-type') || 'text/event-stream' });
        let tail = '';
        for await (const chunk of up.body) { res.write(chunk); tail = (tail + chunk.toString('utf8')).slice(-8000); }
        res.end();
        const us = [...tail.matchAll(/"usage":\{[^}]*\}/g)].map((m) => m[0]);
        log({ id, upstream: 'anthropic', status: up.status, strippedHostedTools: stripped, strippedHostedMcp: hadMcp, usage: us[us.length - 1] || null, errorBody: up.status >= 400 ? tail.slice(0, 300) : undefined });
      })().catch((e) => { log({ id, upstream_error: String(e) }); if (!res.headersSent) json(res, 502, { error: String(e) }); else res.end(); });
      return;
    }
    if (req.method === 'POST' && /\/v1\/messages$/.test(u) && body) return anthropic(body, res);
    if (req.method === 'POST' && /\/responses$/.test(u) && body && ['openai', 'both'].includes(process.env.PASSTHROUGH)) {
      // Real-broker behavior: drop the placeholder credential, add the real one,
      // and refuse provider-hosted tools. The key is never logged.
      const HOSTED = new Set(['web_search', 'web_search_preview', 'file_search', 'code_interpreter', 'image_generation', 'computer_use_preview', 'mcp']);
      const before = (body.tools || []).map((t) => t.type);
      body.tools = (body.tools || []).filter((t) => !HOSTED.has(t.type));
      const stripped = before.filter((t) => HOSTED.has(t));
      (async () => {
        const up = await fetch('https://api.openai.com/v1/responses', { method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer ' + process.env.OPENAI_API_KEY }, body: JSON.stringify(body) });
        res.writeHead(up.status, { 'content-type': up.headers.get('content-type') || 'text/event-stream' });
        let tail = '';
        for await (const chunk of up.body) { res.write(chunk); tail = (tail + chunk.toString('utf8')).slice(-6000); }
        res.end();
        const m = tail.match(/"usage":\{[^}]*\}/);
        log({ id, upstream: 'openai', status: up.status, strippedHostedTools: stripped, usage: m ? m[0] : null });
      })().catch((e) => { log({ id, upstream_error: String(e) }); if (!res.headersSent) json(res, 502, { error: String(e) }); else res.end(); });
      return;
    }
    if (req.method === 'POST' && /\/responses$/.test(u) && body) return responses(body, res);
    if (req.method === 'POST' && u === '/hyalos/submit') { submissions.push(raw); log({ submit: raw }); return json(res, 200, { ok: true }); }
    if (req.method === 'GET' && u === '/__submissions') return json(res, 200, submissions);
    log({ id, unhandled: true });
    json(res, 404, { error: { type: 'not_found', message: 'stub: ' + u } });
  });
});
const ALLOW = (process.env.ALLOW_HOSTS || 'example.com').split(',').map((s) => s.trim()).filter(Boolean);
function allowed(host) { return ALLOW.some((a) => host === a || host.endsWith('.' + a)); }
const faultPath = path.join(dir, 'fault.json');
function takeFault(u) {
  try {
    if (!/\/(v1\/messages|responses)$/.test(u)) return null;
    const f = JSON.parse(fs.readFileSync(faultPath, 'utf8'));
    if (f.count === 0) return null;
    const out = { ...f };
    if (f.count > 0) { f.count -= 1; fs.writeFileSync(faultPath, JSON.stringify(f)); out.count = f.count; }
    return out;
  } catch { return null; }
}
// CONNECT proxy with a host allowlist: the "needed traffic goes through the egress allowlist" path.
server.on('connect', (req, clientSocket, head) => {
  const [host, port] = req.url.split(':'); const ok = allowed(host);
  log({ connect: host, port, allowed: ok });
  if (!ok) { clientSocket.end('HTTP/1.1 403 Forbidden\r\n\r\n'); return; }
  const up = net.connect(Number(port) || 443, host, () => { clientSocket.write('HTTP/1.1 200 Connection Established\r\n\r\n'); if (head?.length) up.write(head); up.pipe(clientSocket); clientSocket.pipe(up); });
  up.on('error', () => clientSocket.end('HTTP/1.1 502 Bad Gateway\r\n\r\n'));
  clientSocket.on('error', () => up.destroy());
});
server.listen(sock, () => { fs.chmodSync(sock, 0o666); console.log('listening', sock); });
