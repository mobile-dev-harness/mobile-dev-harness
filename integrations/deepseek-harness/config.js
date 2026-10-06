import { isAbsolute } from 'node:path';

const FIELDS = new Set(['project', 'command', 'device', 'tools', 'serverName', 'env', 'toolCallTimeoutMs']);

/**
 * @typedef {object} PluginConfig
 * @property {string} project
 * @property {string} [command]
 * @property {string} [device]
 * @property {'core' | 'all'} [tools]
 * @property {string} [serverName]
 * @property {Record<string, string>} [env]
 * @property {number} [toolCallTimeoutMs]
 */

/** @param {unknown} value @param {string} field */
function text(value, field) {
  if (typeof value !== 'string' || !value.trim() || value.includes('\0')) {
    throw new Error(`mobile-dev-harness: ${field} must be a non-empty string without NUL bytes`);
  }
  return value;
}

/**
 * Keep the service bound to one project instead of inheriting the DSH profile's cwd.
 * @param {Partial<PluginConfig>} config
 */
export function mcpConfig(config = {}) {
  if (!config || typeof config !== 'object' || Array.isArray(config)) {
    throw new Error('mobile-dev-harness: config must be an object');
  }
  for (const field of Object.keys(config)) {
    if (!FIELDS.has(field)) throw new Error(`mobile-dev-harness: unknown config field ${field}`);
  }
  const project = text(config.project, 'project (absolute Android project directory)');
  if (!isAbsolute(project)) {
    throw new Error('mobile-dev-harness: project must be an absolute path; ~ is not expanded');
  }
  const command = text(config.command ?? 'mdh', 'command');
  if (!isAbsolute(command) && /[/\\]/.test(command)) {
    throw new Error('mobile-dev-harness: command must be an absolute executable path or a name on PATH');
  }
  const serverName = config.serverName ?? 'mdh';
  if (typeof serverName !== 'string' || !/^[A-Za-z0-9_-]{1,32}$/.test(serverName)) {
    throw new Error('mobile-dev-harness: serverName must be 1–32 letters, digits, underscores or hyphens');
  }
  const tools = config.tools ?? 'core';
  if (tools !== 'core' && tools !== 'all') {
    throw new Error('mobile-dev-harness: tools must be core or all');
  }
  const timeout = config.toolCallTimeoutMs ?? 600_000;
  if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > 2_147_483_647) {
    throw new Error('mobile-dev-harness: toolCallTimeoutMs must be an integer from 1 to 2147483647');
  }
  const env = config.env ?? {};
  if (!env || typeof env !== 'object' || Array.isArray(env)) {
    throw new Error('mobile-dev-harness: env must map environment variable names to strings');
  }
  for (const [key, value] of Object.entries(env)) {
    if (!key || /[=\0]/.test(key) || typeof value !== 'string' || value.includes('\0')) {
      throw new Error('mobile-dev-harness: env must contain valid variable names and string values');
    }
  }
  const args = [];
  if (config.device !== undefined) args.push('--device', text(config.device, 'device'));
  args.push('mcp', '--tools', tools);
  return {
    serverName,
    transport: 'stdio',
    command,
    args,
    cwd: project,
    env: { ...env },
    toolCallTimeoutMs: timeout,
    failOnStartupError: true,
  };
}
