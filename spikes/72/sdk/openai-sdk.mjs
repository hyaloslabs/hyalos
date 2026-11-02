import { Agent, run, tool } from '@openai/agents';
import { z } from 'zod';
const t0 = Date.now();
const submit = tool({
  name: 'submit',
  description: 'Submit the final result to the broker',
  parameters: z.object({ summary: z.string() }),
  async execute({ summary }) {
    const r = await fetch('http://127.0.0.1:8089/hyalos/submit', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ summary }) });
    return await r.text();
  },
});
const agent = new Agent({ name: 'worker', instructions: 'Call the submit tool once with summary "openai agents sdk ok", then stop.', model: 'gpt-5-mini', tools: [submit] });
let out = '', err = null;
try { const r = await run(agent, 'go'); out = r.finalOutput; } catch (e) { err = String(e).slice(0, 300); }
console.log('OPENAI-SDK RESULT:', JSON.stringify(out)?.slice(0, 200), '|', Date.now() - t0, 'ms', err ? '| ERROR ' + err : '');
