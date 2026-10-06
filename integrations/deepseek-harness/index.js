import { stat } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import * as McpClient from '@deepseek-ai/dsh-mcp-client';
import * as SkillFilesystem from '@deepseek-ai/dsh-skill-filesystem';
import { mcpConfig } from './config.js';
import { checkMdhVersion } from './version.js';

export const name = 'mobile-dev-harness';
export const inject = ['tools', 'skills'];

/**
 * @param {{plugin: (plugin: object, config: object) => PromiseLike<unknown>}} ctx
 * @param {Partial<import('./config.js').PluginConfig>} config
 */
export async function apply(ctx, config) {
  const mcp = mcpConfig(config);
  const project = await stat(mcp.cwd).catch(() => null);
  if (!project?.isDirectory()) {
    throw new Error('mobile-dev-harness: project must be an existing directory on the DSH host');
  }
  await checkMdhVersion(mcp);
  // Await discovery before publishing skills that tell the agent to use these tools.
  await ctx.plugin(McpClient, mcp);
  await ctx.plugin(SkillFilesystem, {
    providerName: `mobile-dev-harness-${mcp.serverName}`,
    includeDefaultRoots: false,
    bundledSkillDir: fileURLToPath(new URL('./skills/', import.meta.url)),
    watch: false,
  });
}
