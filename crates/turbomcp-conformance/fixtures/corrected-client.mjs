// Fixture-only corrections to conformance 0.2.0-alpha.11. Assertions are unchanged.
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { existsSync, readFileSync, mkdtempSync, mkdirSync, writeFileSync, symlinkSync, cpSync, rmSync } from 'node:fs';
import { delimiter, dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
let root;
for (const entry of process.env.PATH.split(delimiter)) {
  if (!existsSync(join(entry, 'conformance'))) continue;
  try { root = dirname(createRequire(join(entry, 'probe.cjs')).resolve('@modelcontextprotocol/conformance/package.json')); break; } catch {}
}
if (!root) throw new Error('Run with pnpm --package=@modelcontextprotocol/conformance@0.2.0-alpha.11 dlx node');
let source = readFileSync(join(root, 'dist/index.js'), 'utf8');
const expected = 'a10085d0cfc9dd9192cc227f0f4dd6f1af9a94f6a0d3e30af08d4a0bcf268aae';
if (createHash('sha256').update(source).digest('hex') !== expected) throw new Error('Upstream bundle changed; review fixture corrections');
function replaceOnce(from, to) {
  if (source.split(from).length !== 2) throw new Error(`Fixture patch is not unique: ${from}`);
  source = source.replace(from, to);
}
// The scenario belongs to 2025-11-25; negotiate that version, not 2025-03-26.
replaceOnce('protocolVersion:`2025-03-26`,serverInfo:{name:`sse-retry-test-server`',
            'protocolVersion:`2025-11-25`,serverInfo:{name:`sse-retry-test-server`');
// Modern clients discover; they must not manufacture removed initialization RPCs.
replaceOnce('for(let t of[`initialize`,`notifications/initialized`,`tools/list`,`tools/call`,`resources/list`,`resources/read`,`prompts/list`,`prompts/get`])this.methodHeaderChecks',
            'for(let t of[`server/discover`,`tools/list`,`tools/call`,`resources/list`,`resources/read`,`prompts/list`,`prompts/get`])this.methodHeaderChecks');
// The shared mock previously returned before observing the discovery headers.
replaceOnce('if(r.method===`server/discover`){this.sendDiscover(t,r);return}this.handlePost(e,t,r)',
            'if(r.method===`server/discover`){if(this.name===`http-standard-headers`)this.checkMcpMethodHeader(e,r);this.sendDiscover(t,r);return}this.handlePost(e,t,r)');
const temp = mkdtempSync(join(tmpdir(), 'turbomcp-conformance-fixtures-'));
process.once('exit', () => rmSync(temp, { recursive: true, force: true }));
mkdirSync(join(temp, 'dist'));
writeFileSync(join(temp, 'package.json'), '{"type":"module"}');
writeFileSync(join(temp, 'dist/index.js'), source);
cpSync(join(root, 'requirements'), join(temp, 'requirements'), { recursive: true });
symlinkSync(join(root, '../..'), join(temp, 'node_modules'), 'junction');
console.error(`TurboMCP fixture corrections v1; upstream sha256 ${expected}; assertions unchanged`);
await import(pathToFileURL(join(temp, 'dist/index.js')).href);
