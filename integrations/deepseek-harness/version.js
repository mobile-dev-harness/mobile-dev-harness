import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const run = promisify(execFile);
const hint = 'verify command points to a working mdh executable (requires mdh 0.4.0 or newer)';

/**
 * Check compatibility before MCP startup can obscure an unsupported CLI option.
 * @param {{ command: string, cwd: string, env: Record<string, string> }} config
 */
export async function checkMdhVersion(config) {
  /** @type {Record<string, string>} */
  const env = {};
  // Match DSH's credential/name scrub; --version needs no explicit secrets either.
  for (const [key, value] of Object.entries({ ...process.env, ...config.env })) {
    if (value !== undefined && !/KEY|PASSWORD|SECRET|TOKEN/i.test(key) && !/^DSH_/i.test(key)) {
      env[key] = value;
    }
  }
  let stdout;
  try {
    ({ stdout } = await run(config.command, ['--version'], {
      cwd: config.cwd, env, shell: false, encoding: 'utf8',
      timeout: 5_000, maxBuffer: 4_096, killSignal: 'SIGKILL',
    }));
  } catch (error) {
    const failure = /** @type {import('node:child_process').ExecFileException} */ (error);
    if (failure.code === 'ENOENT') {
      throw new Error('mobile-dev-harness: cannot find mdh; install mdh 0.4.0 or newer and set command to its executable path or update PATH');
    }
    if (failure.code === 'EACCES' || failure.code === 'EPERM') {
      throw new Error('mobile-dev-harness: cannot execute mdh; check executable permissions and the configured command');
    }
    if (failure.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER') {
      throw new Error(`mobile-dev-harness: mdh --version exceeded the output limit; ${hint}`);
    }
    if (failure.killed) {
      throw new Error(`mobile-dev-harness: mdh --version timed out after 5 seconds; ${hint}`);
    }
    throw new Error(`mobile-dev-harness: mdh --version failed; ${hint}`);
  }
  const match = /^mdh ((0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)$/.exec(stdout.trim());
  if (!match || match[5]?.split('.').some(part => /^0\d+$/.test(part))) {
    throw new Error(`mobile-dev-harness: unrecognized mdh --version output; ${hint}`);
  }
  const [, version, major, minor, patch, prerelease] = match;
  if (BigInt(major) === 0n && (BigInt(minor) < 4n || (BigInt(minor) === 4n && BigInt(patch) === 0n && prerelease))) {
    throw new Error('mobile-dev-harness: this integration requires mdh 0.4.0 or newer; upgrade mdh and check the configured command');
  }
  return version;
}
