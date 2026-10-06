import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { access, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import { listMcpTools } from './mcp-contract.mjs';

// Install only our packed artifact offline; use the existing, built DSH checkout for peers.
const run = promisify(execFile);
const packageDir = fileURLToPath(new URL('../', import.meta.url));
const skills = ['mdh-compat', 'mdh-debug-crash', 'mdh-perf', 'mdh-verify', 'mdh-visual'];

function requiredPath(name) {
  const value = process.env[name];
  assert.ok(value && isAbsolute(value),
    `Set ${name} to an absolute path. Run DSH_ROOT=/path/to/built/deepseek-harness DSH_MDH_BINARY=/path/to/mdh npm run test:dsh.`);
  return value;
}

async function command(file, args, options = {}) {
  return run(file, args, { timeout: 60_000, maxBuffer: 2 * 1024 * 1024, ...options });
}

async function fixture(project) {
  const files = {
    'settings.gradle.kts': 'include(":app")\n',
    'app/build.gradle.kts': 'plugins { id("com.android.application") }\n',
    'app/src/main/AndroidManifest.xml': `<manifest xmlns:android="http://schemas.android.com/apk/res/android">
  <application><activity android:name=".DshSmokeActivity" android:exported="true" /></application>
</manifest>\n`,
    'app/src/main/kotlin/dev/smoke/DshSmokeActivity.kt': `package dev.smoke
class DshSmokeActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        title = "Before"
    }
}
`,
  };
  for (const [path, content] of Object.entries(files)) {
    const destination = join(project, path);
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, content);
  }
  const git = (...args) => command('git', [
    '-c', 'user.name=DSH integration test', '-c', 'user.email=dsh-test@example.invalid',
    '-c', 'commit.gpgsign=false', '-c', 'core.hooksPath=/dev/null', ...args,
  ], { cwd: project });
  await git('init', '-q');
  await git('add', '.');
  await git('commit', '-qm', 'Create isolated Android fixture');
  const source = 'app/src/main/kotlin/dev/smoke/DshSmokeActivity.kt';
  await writeFile(join(project, source), files[source].replace('Before', 'Changed by the DSH smoke test'));
}

function assertTools(ctx, serverName, names) {
  assert.deepEqual(ctx.tools.schemas().map(tool => tool.name).sort(),
    names.map(name => `mcp__${serverName}__${name}`).sort());
}

async function assertSkills(ctx) {
  assert.deepEqual((await ctx.skills.list()).map(skill => skill.name).sort(), skills);
  for (const name of skills) {
    const skill = await ctx.skills.get(name);
    assert.ok(skill.content.length > 100, `${name} has a loadable body`);
    assert.equal(skill.source, 'bundled');
  }
}

async function impact(ctx, namespace, callId) {
  const result = await ctx.tools.execute({
    signal: new AbortController().signal,
    callId,
    name: `mcp__${namespace}__mdh_impact`,
    arguments: {},
  });
  assert.equal(result.isError, false, JSON.stringify(result));
  // The MCP bridge keeps the original result and projects its text for the model.
  assert.notEqual(result.value.isError, true, JSON.stringify(result.value));
  const text = result.content.filter(block => block.type === 'text').map(block => block.text).join('\n');
  assert.match(text, /1 file changed/, 'impact must observe the fixture edit');
  assert.match(text, /DshSmokeActivity/, 'impact without a project argument must analyze the configured cwd');
  assert.doesNotMatch(text, /no changes against HEAD/, 'the fixture contains an actual uncommitted Kotlin edit');
}

async function main() {
  const dshRoot = requiredPath('DSH_ROOT');
  const binary = requiredPath('DSH_MDH_BINARY');
  await access(binary, constants.X_OK);
  const coreTools = (await listMcpTools(binary, 'core')).map(tool => tool.name);
  const allTools = (await listMcpTools(binary, 'all')).map(tool => tool.name);
  const load = async (path) => {
    const file = join(dshRoot, path, 'lib/index.js');
    await access(file).catch(() => {
      throw new Error(`Missing built DSH module: ${file}. Build the DSH checkout before running test:dsh.`);
    });
    return import(pathToFileURL(file).href);
  };
  const { Context } = await load('vendor/cordis');
  const { default: SystemPrompt } = await load('packages/core/system-prompt');
  const { default: ToolRuntime } = await load('packages/core/tools');
  const { default: SkillRegistry } = await load('packages/skill/skill');
  const { default: Loader } = await load('vendor/loader');
  const { applyEntryPatches } = await load('vendor/include');
  const { loadOverlayPatches } = await load('packages/boot/app-boot');
  const temporary = await mkdtemp(join(tmpdir(), 'mdh-dsh-smoke-'));
  let ctx;
  try {
    const packed = JSON.parse((await command('npm', [
      'pack', '--ignore-scripts', '--json', '--pack-destination', temporary,
    ], { cwd: packageDir, env: { ...process.env, npm_config_cache: join(temporary, 'npm-cache') } })).stdout)[0];
    const paths = packed.files.map(file => file.path);
    for (const path of ['index.js', 'config.js', 'version.js', 'cordis.patch.yml', 'LICENSE-MIT', 'LICENSE-APACHE', ...skills.map(name => `skills/${name}/SKILL.md`)]) {
      assert.ok(paths.includes(path), `${path} must be in the published package`);
    }
    assert.ok(!paths.some(path => path.startsWith('tests/')), 'test fixtures must not be published');
    const installation = join(temporary, 'install');
    await command('npm', [
      'install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund',
      '--package-lock=false', '--prefix', installation, join(temporary, packed.filename),
    ], { env: { ...process.env, npm_config_cache: join(temporary, 'npm-cache') } });
    const extracted = join(installation, 'node_modules', packed.name);
    const manifest = JSON.parse(await readFile(join(extracted, 'package.json'), 'utf8'));
    assert.equal(manifest.license, 'MIT OR Apache-2.0');
    assert.equal(manifest.dsh.bundle.patch, './cordis.patch.yml');
    const peers = join(extracted, 'node_modules', '@deepseek-ai');
    await mkdir(peers, { recursive: true });
    for (const [name, path] of [
      ['dsh-mcp-client', 'packages/mcp/mcp-client'],
      ['dsh-skill-filesystem', 'packages/skill/skill-filesystem'],
    ]) {
      await access(join(dshRoot, path, 'lib/index.js'));
      await symlink(join(dshRoot, path), join(peers, name), 'dir');
    }
    const project = join(temporary, 'Android project with spaces');
    await fixture(project);
    const config = { project, command: binary };
    const patches = loadOverlayPatches('test:dsh', join(extracted, 'cordis.patch.yml'));
    const warn = message => assert.fail(`Unexpected patch warning: ${message}`);
    const disabled = applyEntryPatches([], patches, warn);
    assert.equal(disabled.length, 1);
    assert.equal(disabled[0].disabled, true, 'installation must wait for explicit project configuration');
    assert.equal(disabled[0].name, manifest.name);
    const configured = applyEntryPatches([], [...patches, {
      id: 'mobile-dev-harness', disabled: false, config,
    }], warn);
    assert.equal(configured[0].disabled, false);
    assert.deepEqual(configured[0].config, config);
    const exports = await import(pathToFileURL(join(extracted, 'index.js')).href);
    const adapter = Loader.prototype.unwrapExports(exports);
    assert.equal(adapter.apply, exports.apply, 'the real loader must retain the namespace plugin');
    ctx = new Context();
    await ctx.plugin(SystemPrompt, {});
    await ctx.plugin(ToolRuntime, {});
    await ctx.plugin(SkillRegistry, {});
    await assert.rejects(async () => {
      await ctx.plugin(adapter, { ...config, project: join(temporary, 'missing-project') });
    }, /project must be an existing directory/);
    assertTools(ctx, 'mdh', []);
    assert.deepEqual(await ctx.skills.list(), []);
    await assert.rejects(async () => {
      await ctx.plugin(adapter, { ...config, command: join(temporary, 'missing-mdh') });
    }, /cannot find mdh/);
    assertTools(ctx, 'mdh', []);
    assert.deepEqual(await ctx.skills.list(), []);

    let fiber = ctx.plugin(adapter, configured[0].config);
    await fiber;
    assertTools(ctx, 'mdh', coreTools);
    await assertSkills(ctx);
    await impact(ctx, 'mdh', 'default-impact');
    await fiber.dispose();
    assertTools(ctx, 'mdh', []);
    assert.deepEqual(await ctx.skills.list(), [], 'disposing the adapter must remove its skill provider');

    fiber = ctx.plugin(adapter, { ...config, tools: 'all' });
    await fiber;
    assertTools(ctx, 'mdh', allTools);
    await assertSkills(ctx);
    await impact(ctx, 'mdh', 'all-impact');
    await fiber.dispose();
    assertTools(ctx, 'mdh', []);
    assert.deepEqual(await ctx.skills.list(), []);

    fiber = ctx.plugin(adapter, { ...config, serverName: 'android-test' });
    await fiber;
    assertTools(ctx, 'android-test', coreTools);
    await assertSkills(ctx);
    await impact(ctx, 'android-test', 'custom-namespace-impact');
    await fiber.dispose();
    assertTools(ctx, 'android-test', []);
    assert.deepEqual(await ctx.skills.list(), []);
    console.log(`PASS: offline npm install of packed bundle, real DSH patch/loader APIs, ${coreTools.length} default core tools, ${allTools.length} opt-in tools, 5 skills, project cwd, custom namespace, startup failure rollback, disposal and remount.`);
  } finally {
    try {
      if (ctx) await ctx.fiber.dispose();
    } finally {
      await rm(temporary, { recursive: true, force: true });
    }
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
