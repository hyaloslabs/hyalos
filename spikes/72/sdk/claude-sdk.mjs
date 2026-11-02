import { query } from '@anthropic-ai/claude-agent-sdk';
const t0 = Date.now();
let result = '', turns = 0, err = null;
try {
  for await (const m of query({
    prompt: 'Run: hyalos-submit "{\\"summary\\":\\"claude agent sdk ok\\"}" and stop.',
    options: { model: 'claude-haiku-5-5', tools: ['Bash'], allowedTools: ['Bash'], cwd: '/tmp', env: { ...process.env }, settingSources: [] },
  })) {
    if (m.type === 'assistant') turns++;
    if (m.type === 'result') result = m.result ?? JSON.stringify(m).slice(0, 200);
  }
} catch (e) { err = String(e).slice(0, 300); }
console.log('CLAUDE-SDK RESULT:', JSON.stringify(result).slice(0, 200), '| assistant turns:', turns, '|', Date.now() - t0, 'ms', err ? '| ERROR ' + err : '');
