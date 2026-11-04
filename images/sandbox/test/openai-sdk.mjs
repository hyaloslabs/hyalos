// The image test's OpenAI Agents SDK run (ADR-0040): one agent run against the scripted model.
import { Agent, run } from '@openai/agents';
const agent = new Agent({ name: 'worker', instructions: 'Answer ok.', model: 'scripted' });
const r = await run(agent, 'say ok');
console.log('openai-sdk:', JSON.stringify(r.finalOutput));
process.exit(r.finalOutput ? 0 : 1);
