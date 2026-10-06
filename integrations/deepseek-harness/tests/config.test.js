import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mcpConfig } from '../config.js';

test('pins project cwd and defaults to the compact core tool set', () => {
  const config = mcpConfig({ project: '/projects/Android App' });
  assert.equal(config.cwd, '/projects/Android App');
  assert.equal(config.command, 'mdh');
  assert.deepEqual(config.args, ['mcp', '--tools', 'core']);
  assert.equal(config.transport, 'stdio');
  assert.equal(config.serverName, 'mdh');
  assert.equal(config.failOnStartupError, true);
  assert.equal(config.toolCallTimeoutMs, 600_000);
});

test('preserves literal arguments and explicitly forwarded environment', () => {
  const env = { MDH_PASSWORD: 'literal $(value)', EMPTY: '' };
  const config = mcpConfig({
    project: '/projects/app', command: '/tools/with spaces/mdh',
    device: '127.0.0.1:5555', tools: 'all', serverName: 'mdh_test',
    env, toolCallTimeoutMs: 12_000,
  });
  assert.equal(config.command, '/tools/with spaces/mdh');
  assert.deepEqual(config.args, ['--device', '127.0.0.1:5555', 'mcp', '--tools', 'all']);
  assert.deepEqual(config.env, env);
  config.env.EMPTY = 'changed';
  assert.equal(env.EMPTY, '');
  assert.equal(config.toolCallTimeoutMs, 12_000);
});

test('requires an explicit absolute project and executable path', () => {
  for (const project of [undefined, '', '   ', '.', '~/app', 'app', 123, '/app\0']) {
    assert.throws(() => mcpConfig({ project }), /project/);
  }
  for (const command of ['', './mdh', '../bin/mdh', 'bin/mdh', 123, 'mdh\0']) {
    assert.throws(() => mcpConfig({ project: '/app', command }), /command/);
  }
});

test('rejects configuration typos and invalid namespaces before spawning', () => {
  for (const config of [null, [], 'text']) assert.throws(() => mcpConfig(config), /config/);
  assert.throws(() => mcpConfig({ project: '/app', cwd: '/wrong' }), /unknown config field cwd/);
  for (const serverName of ['', 'a b', 'a/b', 'a'.repeat(33), 123]) {
    assert.throws(() => mcpConfig({ project: '/app', serverName }), /serverName/);
  }
  assert.throws(() => mcpConfig({ project: '/app', tools: 'everything' }), /tools/);
  assert.throws(() => mcpConfig({ project: '/app', device: '' }), /device/);
});

test('rejects invalid timers and environments without exposing secret values', () => {
  for (const toolCallTimeoutMs of [0, -1, 1.5, Infinity, NaN, 2_147_483_648, '60000']) {
    assert.throws(() => mcpConfig({ project: '/app', toolCallTimeoutMs }), /toolCallTimeoutMs/);
  }
  for (const env of [[], 'secret', { SECRET: 123 }, { 'BAD=KEY': 'secret' }, { SECRET: 'secret\0' }]) {
    assert.throws(() => mcpConfig({ project: '/app', env }), error => {
      assert.match(error.message, /env/);
      assert.ok(!error.message.includes('secret'));
      return true;
    });
  }
});
