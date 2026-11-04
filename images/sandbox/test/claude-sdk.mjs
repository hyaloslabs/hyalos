// The image test's Claude Agent SDK run (ADR-0040): one query against the scripted model.
import { query } from '@anthropic-ai/claude-agent-sdk';
let result = null;
for await (const m of query({ prompt: 'say ok', options: { tools: ['Bash'], cwd: '/tmp', settingSources: [] } })) {
  if (m.type === 'result') result = m;
}
console.log('claude-sdk:', result?.subtype, JSON.stringify(result?.result ?? null));
process.exit(result?.subtype === 'success' ? 0 : 1);
