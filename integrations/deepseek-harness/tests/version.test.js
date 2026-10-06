import assert from 'node:assert/strict';
import { chmod, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkMdhVersion } from '../version.js';

async function fixture(t, body) {
  const directory = await mkdtemp(join(tmpdir(), 'mdh version with spaces '));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const command = join(directory, 'mdh');
  await writeFile(command, `#!${process.execPath}\n${body}\n`, { mode: 0o755 });
  return { command, cwd: directory, env: {} };
}

test('accepts supported numeric versions and build metadata', async t => {
  await Promise.all(['0.4.0', '0.4.1', '0.10.0', '1.0.0', '0.4.0+build.7'].map(async version => {
    const config = await fixture(t, `console.log('mdh ${version}');`);
    assert.equal(await checkMdhVersion(config), version);
  }));
});

test('rejects older releases and prereleases below the minimum', async t => {
  await Promise.all(['0.3.0', '0.3.99', '0.4.0-alpha', '0.4.0-rc.1'].map(async version => {
    const config = await fixture(t, `console.log('mdh ${version}');`);
    await assert.rejects(checkMdhVersion(config), /requires mdh 0\.4\.0 or newer.*upgrade/);
  }));
});

test('rejects malformed output and the wrong executable without leaking output', async t => {
  await Promise.all(['other 1.0.0', 'mdh 04.0.0', 'mdh 0.4', 'mdh 0.4.0-01', 'private-output', 'mdh 0.4.0\nprivate-output'].map(async output => {
    const config = await fixture(t, `console.log(${JSON.stringify(output)});`);
    await assert.rejects(checkMdhVersion(config), error => {
      assert.match(error.message, /command.*mdh executable/);
      assert.doesNotMatch(error.message, /private-output/);
      return true;
    });
  }));
});

test('checks literal version arguments and preserves safe environment overrides', async t => {
  const inheritedSecret = process.env.MDH_TEST_INHERITED_TOKEN;
  process.env.MDH_TEST_INHERITED_TOKEN = 'private-inherited-secret';
  t.after(() => {
    if (inheritedSecret === undefined) delete process.env.MDH_TEST_INHERITED_TOKEN;
    else process.env.MDH_TEST_INHERITED_TOKEN = inheritedSecret;
  });
  const config = await fixture(t, `
    const assert = require('node:assert/strict');
    assert.deepEqual(process.argv.slice(2), ['--version']);
    assert.equal(process.cwd(), ${JSON.stringify(await realpath(tmpdir()))});
    assert.equal(process.env.JAVA_HOME, '/literal/$(value)');
    assert.equal(process.env.MDH_PASSWORD, undefined);
    assert.equal(process.env.DEEPSEEK_API_KEY, undefined);
    assert.equal(process.env.dsh_identity, undefined);
    assert.equal(process.env.custom_secret, undefined);
    assert.equal(process.env.MDH_TEST_INHERITED_TOKEN, undefined);
    console.log('mdh 0.4.0');
  `);
  config.env = {
    PATH: join(config.cwd), JAVA_HOME: '/literal/$(value)', MDH_PASSWORD: 'private-secret',
    DEEPSEEK_API_KEY: 'private-secret', dsh_identity: 'private-secret', custom_secret: 'private-secret',
  };
  config.command = 'mdh';
  config.cwd = tmpdir();
  assert.equal(await checkMdhVersion(config), '0.4.0');
});

test('reports missing and non-executable commands with an actionable hint', async t => {
  const config = await fixture(t, "console.log('mdh 0.4.0');");
  await assert.rejects(checkMdhVersion({ ...config, command: join(config.cwd, 'missing') }), /cannot find mdh.*install.*command/);
  await chmod(config.command, 0o644);
  await assert.rejects(checkMdhVersion(config), /cannot execute mdh.*permissions/);
});

test('hides subprocess diagnostics when version exits unsuccessfully', async t => {
  const config = await fixture(t, "console.error('private-secret'); console.log('private-output'); process.exit(1);");
  await assert.rejects(checkMdhVersion(config), error => {
    assert.match(error.message, /mdh --version failed.*command/);
    assert.doesNotMatch(error.message, /private-/);
    return true;
  });
});

test('bounds version output without leaking it', async t => {
  const config = await fixture(t, "console.log('private-secret'.repeat(1000));");
  await assert.rejects(checkMdhVersion(config), error => {
    assert.match(error.message, /mdh --version.*output limit.*command/);
    assert.doesNotMatch(error.message, /private-secret/);
    return true;
  });
});

test('terminates a hanging version process and gives a timeout hint', async t => {
  const config = await fixture(t, 'setInterval(() => {}, 1000);');
  await assert.rejects(checkMdhVersion(config), /mdh --version timed out.*command/);
});
