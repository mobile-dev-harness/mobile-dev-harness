import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { access, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
import { readSkillContracts, validateSkillContracts } from './skill-contract.js';

/** Discover the binary's actual public API without making device-dependent tool calls. */
export async function listMcpTools(binary, mode) {
  const cwd = await mkdtemp(join(tmpdir(), 'mdh-mcp-contract-'));
  const child = spawn(binary, ['mcp', '--tools', mode], { cwd, stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  let nextId = 1;
  let failure;
  let stderr = '';
  const fail = error => {
    failure = error;
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
  };
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4096); });
  child.on('error', fail);
  child.stdin.on('error', fail);
  const closed = new Promise(resolveClose => child.once('close', (code, signal) => {
    fail(new Error(`mdh ${mode} exited (${code ?? signal}) before completing schema discovery${stderr ? `: ${stderr}` : ''}`));
    resolveClose();
  }));
  const lines = createInterface({ input: child.stdout });
  lines.on('line', line => {
    try {
      const message = JSON.parse(line);
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id);
      if (message.error) request.reject(new Error(`${request.method}: ${JSON.stringify(message.error)}`));
      else if (!Object.hasOwn(message, 'result')) request.reject(new Error(`${request.method}: missing JSON-RPC result`));
      else request.resolve(message.result);
    } catch (error) {
      fail(new Error(`invalid MCP stdout: ${error.message}`));
    }
  });
  const send = message => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
  const request = (method, params) => new Promise((resolveRequest, reject) => {
    if (failure) return reject(failure);
    const id = nextId++;
    pending.set(id, { method, resolve: resolveRequest, reject });
    send({ id, method, params });
  });
  const deadline = setTimeout(() => {
    fail(new Error(`mdh ${mode} schema discovery timed out after 15 seconds`));
    child.kill('SIGKILL');
  }, 15_000);
  try {
    const initialized = await request('initialize', {
      protocolVersion: '2024-11-05', capabilities: {},
      clientInfo: { name: 'mdh-skill-contract', version: '1.0.0' },
    });
    assert.equal(typeof initialized.protocolVersion, 'string');
    send({ method: 'notifications/initialized' });
    const tools = [];
    const cursors = new Set();
    let cursor;
    do {
      const page = await request('tools/list', cursor ? { cursor } : {});
      assert.ok(Array.isArray(page.tools), 'tools/list must return a tools array');
      tools.push(...page.tools);
      cursor = page.nextCursor;
      if (cursor) {
        assert.ok(!cursors.has(cursor), 'tools/list repeated a pagination cursor');
        cursors.add(cursor);
      }
    } while (cursor);
    assert.ok(tools.length, `${mode} must advertise tools`);
    assert.equal(new Set(tools.map(tool => tool.name)).size, tools.length, `${mode} has duplicate tool names`);
    return tools;
  } finally {
    clearTimeout(deadline);
    child.stdin.end();
    const terminate = setTimeout(() => child.kill('SIGTERM'), 1000);
    const kill = setTimeout(() => child.kill('SIGKILL'), 2000);
    try {
      await closed;
    } finally {
      clearTimeout(terminate);
      clearTimeout(kill);
      lines.close();
      await rm(cwd, { recursive: true, force: true });
    }
  }
}

async function main() {
  const binary = process.env.DSH_MDH_BINARY;
  assert.ok(binary && isAbsolute(binary), 'Set DSH_MDH_BINARY to the absolute path of a compiled mdh executable, then run npm run test:contract.');
  await access(binary, constants.X_OK);
  const all = await listMcpTools(binary, 'all');
  const core = await listMcpTools(binary, 'core');
  assert.ok(core.length < all.length, 'core must be a strict subset of all');
  for (const tool of core) {
    assert.deepEqual(tool, all.find(candidate => candidate.name === tool.name), `${tool.name}: core and all must expose the same contract`);
  }
  const report = validateSkillContracts(await readSkillContracts(), all);
  console.log(`PASS: ${report.skills} skills and ${report.examples} MCP examples match ${all.length} live tools; ${core.length} core tools are a consistent subset. No device or DSH runtime used.`);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
