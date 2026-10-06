import { readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const annotations = new Set(['$schema', '$id', '$comment', 'title', 'description', 'default', 'examples', 'deprecated', 'readOnly', 'writeOnly']);
const keywords = new Set(['$ref', '$defs', 'definitions', 'type', 'properties', 'required', 'additionalProperties', 'items', 'enum', 'const', 'anyOf', 'oneOf']);
const types = new Set(['object', 'array', 'string', 'number', 'integer', 'boolean', 'null']);
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);

export function normalizeToolName(name) {
  if (typeof name !== 'string') throw new Error('tool must be a string');
  const match = /^(?:mcp__[A-Za-z0-9_-]+__)?(mdh_[A-Za-z0-9_]+)$/.exec(name);
  if (!match) throw new Error(`invalid mdh tool name: ${name}`);
  return match[1];
}

/** Named references and executable examples share the same source and line diagnostics. */
export function collectSkillContract(markdown, source = '<skill>') {
  const lines = markdown.split(/\r?\n/);
  const references = [];
  const calls = [];
  for (const [index, line] of lines.entries()) {
    for (const match of line.matchAll(/\b(?:mcp__[A-Za-z0-9_-]+__)?mdh_[A-Za-z0-9_]+\b/g)) {
      references.push({ tool: normalizeToolName(match[0]), line: index + 1 });
    }
  }
  for (let index = 0; index < lines.length; index++) {
    const opening = /^\s*(`{3,}|~{3,})mcp\s*$/.exec(lines[index]);
    if (!opening) continue;
    const start = index + 1;
    const closing = new RegExp(`^\\s*${opening[1][0]}{${opening[1].length},}\\s*$`);
    const body = [];
    while (++index < lines.length && !closing.test(lines[index])) body.push(lines[index]);
    if (index === lines.length) throw new Error(`${source}:${start}: unclosed mcp fence`);
    let call;
    try {
      call = JSON.parse(body.join('\n'));
      if (!object(call) || Object.keys(call).sort().join(',') !== 'arguments,tool' || !object(call.arguments)) {
        throw new Error('expected one {"tool":"mdh_name","arguments":{...}} object');
      }
      call.tool = normalizeToolName(call.tool);
    } catch (error) {
      throw new Error(`${source}:${start}: invalid mcp example: ${error.message}`);
    }
    calls.push({ ...call, line: start });
  }
  return { source, references, calls };
}

function resolveRef(root, ref) {
  if (typeof ref !== 'string' || !ref.startsWith('#/')) throw new Error(`unsupported schema $ref: ${ref}`);
  let target = root;
  for (const key of ref.slice(2).split('/').map(part => part.replace(/~1/g, '/').replace(/~0/g, '~'))) {
    if (!object(target) || !Object.hasOwn(target, key)) throw new Error(`unresolved schema $ref: ${ref}`);
    target = target[key];
  }
  return target;
}

/** Reject new schema features until this deliberately small contract checker supports them. */
export function assertSupportedSchema(schema, root = schema, seen = new Set()) {
  if (typeof schema === 'boolean' || seen.has(schema)) return;
  if (!object(schema)) throw new Error('schema must be an object or boolean');
  seen.add(schema);
  for (const key of Object.keys(schema)) {
    if (!annotations.has(key) && !keywords.has(key)) throw new Error(`unsupported schema keyword: ${key}`);
  }
  if (schema.$ref !== undefined) assertSupportedSchema(resolveRef(root, schema.$ref), root, seen);
  if (schema.type !== undefined) {
    const declared = Array.isArray(schema.type) ? schema.type : [schema.type];
    if (!declared.length || declared.some(type => !types.has(type))) throw new Error(`unsupported schema type: ${schema.type}`);
  }
  if (schema.required !== undefined && (!Array.isArray(schema.required) || schema.required.some(key => typeof key !== 'string'))) {
    throw new Error('schema required must contain property names');
  }
  if (schema.enum !== undefined && (!Array.isArray(schema.enum) || !schema.enum.length)) throw new Error('schema enum must be a non-empty array');
  for (const key of ['properties', '$defs', 'definitions']) {
    if (schema[key] === undefined) continue;
    if (!object(schema[key])) throw new Error(`schema ${key} must be an object`);
    for (const child of Object.values(schema[key])) assertSupportedSchema(child, root, seen);
  }
  for (const key of ['items', 'additionalProperties']) {
    if (schema[key] !== undefined) assertSupportedSchema(schema[key], root, seen);
  }
  for (const key of ['anyOf', 'oneOf']) {
    if (schema[key] === undefined) continue;
    if (!Array.isArray(schema[key]) || !schema[key].length) throw new Error(`schema ${key} must be a non-empty array`);
    for (const child of schema[key]) assertSupportedSchema(child, root, seen);
  }
}

function matchesType(value, type) {
  if (type === 'null') return value === null;
  if (type === 'object') return object(value);
  if (type === 'array') return Array.isArray(value);
  if (type === 'integer') return Number.isInteger(value);
  if (type === 'number') return typeof value === 'number' && Number.isFinite(value);
  return typeof value === type;
}

function errorsFor(value, schema, root, path, depth = 0) {
  if (depth > 100) throw new Error('schema recursion exceeded 100 levels');
  if (schema === true) return [];
  if (schema === false) return [`${path}: value is forbidden`];
  const errors = [];
  const childErrors = (entry, child, childPath = path) => errorsFor(entry, child, root, childPath, depth + 1);
  if (schema.$ref) errors.push(...childErrors(value, resolveRef(root, schema.$ref)));
  if (schema.type !== undefined) {
    const declared = Array.isArray(schema.type) ? schema.type : [schema.type];
    if (!declared.some(type => matchesType(value, type))) return [...errors, `${path}: expected ${declared.join(' or ')}`];
  }
  if (schema.enum && !schema.enum.some(option => isDeepStrictEqual(value, option))) errors.push(`${path}: expected enum ${JSON.stringify(schema.enum)}`);
  if (Object.hasOwn(schema, 'const') && !isDeepStrictEqual(value, schema.const)) errors.push(`${path}: expected const ${JSON.stringify(schema.const)}`);
  for (const key of ['anyOf', 'oneOf']) {
    if (!schema[key]) continue;
    const branches = schema[key].map(branch => childErrors(value, branch));
    const count = branches.filter(branch => !branch.length).length;
    if (key === 'oneOf' ? count !== 1 : count === 0) {
      errors.push(`${path}: ${key} matched ${count} branches; ${branches.flat().join('; ')}`);
    }
  }
  if (Array.isArray(value) && schema.items !== undefined) {
    value.forEach((entry, index) => errors.push(...childErrors(entry, schema.items, `${path}[${index}]`)));
  }
  if (object(value)) {
    for (const key of schema.required ?? []) {
      if (!Object.hasOwn(value, key)) errors.push(`${path}.${key}: required property is missing`);
    }
    // Rust ignores unknown fields by default; examples must still reject misspelled parameters.
    const plainObject = schema.type === 'object' && !schema.$ref && !schema.anyOf && !schema.oneOf;
    if (schema.properties || plainObject || schema.additionalProperties !== undefined) {
      for (const [key, entry] of Object.entries(value)) {
        const child = Object.hasOwn(schema.properties ?? {}, key) ? schema.properties[key] : schema.additionalProperties ?? false;
        if (child === false) errors.push(`${path}.${key}: unknown property`);
        else errors.push(...childErrors(entry, child, `${path}.${key}`));
      }
    }
  }
  return errors;
}

export function validateArguments(value, schema) {
  assertSupportedSchema(schema);
  const errors = errorsFor(value, schema, schema, 'arguments');
  if (errors.length) throw new Error(errors.join('\n'));
}

export function validateSkillContracts(contracts, tools) {
  const catalog = new Map();
  for (const tool of tools) {
    if (catalog.has(tool.name)) throw new Error(`duplicate tool schema: ${tool.name}`);
    assertSupportedSchema(tool.inputSchema);
    catalog.set(tool.name, tool.inputSchema);
  }
  for (const { source, references, calls } of contracts) {
    for (const { tool, line } of [...references, ...calls]) {
      if (!catalog.has(tool)) throw new Error(`${source}:${line}: unknown tool ${tool}`);
    }
    if (!calls.length) throw new Error(`${source}: add at least one executable fenced mcp example`);
    for (const call of calls) {
      try {
        validateArguments(call.arguments, catalog.get(call.tool));
      } catch (error) {
        throw new Error(`${source}:${call.line}: ${call.tool}: ${error.message}`);
      }
    }
  }
  return { skills: contracts.length, examples: contracts.reduce((total, contract) => total + contract.calls.length, 0) };
}

export async function readSkillContracts(roots = [
  fileURLToPath(new URL('../../claude-code/skills/', import.meta.url)),
  fileURLToPath(new URL('../skills/', import.meta.url)),
]) {
  const contracts = [];
  for (const root of roots) {
    const directories = (await readdir(root, { withFileTypes: true })).filter(entry => entry.isDirectory()).sort((a, b) => a.name.localeCompare(b.name));
    if (!directories.length) throw new Error(`no skill directories found: ${root}`);
    for (const directory of directories) {
      const path = join(root, directory.name, 'SKILL.md');
      contracts.push(collectSkillContract(await readFile(path, 'utf8'), path));
    }
  }
  return contracts;
}
