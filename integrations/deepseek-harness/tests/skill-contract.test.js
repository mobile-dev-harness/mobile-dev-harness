import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertSupportedSchema, collectSkillContract, normalizeToolName, validateArguments, validateSkillContracts } from './skill-contract.js';

const act = {
  name: 'mdh_act',
  inputSchema: {
    type: 'object', required: ['actions'], properties: {
      actions: { type: 'array', items: {
        type: 'object', required: ['action'], properties: {
          action: { type: 'string', enum: ['tap', 'type'] },
          target: { type: 'string' },
        },
      } },
    },
  },
};
const example = argumentsValue => `Use \`mcp__mdh__mdh_act\` (other \`mdh_*\` tools are separate).\n\n\`\`\`mcp\n${JSON.stringify({ tool: 'mdh_act', arguments: argumentsValue })}\n\`\`\`\n`;
const validate = markdown => validateSkillContracts([collectSkillContract(markdown, 'skill.md')], [act]);

test('collects canonical and DSH-qualified names, ignores wildcards, and retains source lines', () => {
  assert.equal(normalizeToolName('mcp__android-test__mdh_act'), 'mdh_act');
  const contract = collectSkillContract(example({ actions: [{ action: 'tap', target: 'Sign in' }] }), 'skill.md');
  assert.deepEqual(contract.references.map(reference => reference.tool), ['mdh_act', 'mdh_act']);
  assert.equal(contract.calls[0].line, 3);
  assert.deepEqual(validateSkillContracts([contract], [act]), { skills: 1, examples: 1 });
});

test('removed tools in prose or executable calls fail with file and line', () => {
  assert.throws(() => validate(`Use \`mdh_removed\`.\n${example({ actions: [] })}`), /skill.md:1: unknown tool mdh_removed/);
  assert.throws(() => validate(example({ actions: [] }).replace('"mdh_act"', '"mdh_old"')), /unknown tool mdh_old/);
});

test('renamed parameters cannot be silently ignored by Rust deserialization', () => {
  assert.throws(() => validate(example({ steps: [] })), /arguments.steps: unknown property/);
  assert.throws(() => validate(example({})), /arguments.actions: required property is missing/);
});

test('nested action enums, misspelled fields, item types and required fields are checked', () => {
  assert.throws(() => validate(example({ actions: [{ action: 'click' }] })), /arguments.actions\[0\].action: expected enum/);
  assert.throws(() => validate(example({ actions: [{ action: 'tap', selector: 'Sign in' }] })), /arguments.actions\[0\].selector: unknown property/);
  assert.throws(() => validate(example({ actions: [{ target: 'Sign in' }] })), /arguments.actions\[0\].action: required property/);
  assert.throws(() => validate(example({ actions: ['tap'] })), /arguments.actions\[0\]: expected object/);
  assert.throws(() => validate(example({ actions: 'tap' })), /arguments.actions: expected array/);
});

test('malformed, unclosed and missing executable examples fail instead of being skipped', () => {
  assert.throws(() => collectSkillContract('```mcp\nnot JSON\n```', 'skill.md'), /skill.md:1: invalid mcp example/);
  assert.throws(() => collectSkillContract('```mcp\n{}', 'skill.md'), /unclosed mcp fence/);
  assert.throws(() => collectSkillContract('```mcp\n{"tool":"mdh_act","arguments":{},"comment":"x"}\n```'), /expected one/);
  assert.throws(() => validate('Use `mdh_act`.'), /add at least one executable fenced mcp example/);
});

test('local definitions, nullable types and anyOf are validated recursively', () => {
  const schema = {
    type: 'object', properties: { value: { $ref: '#/$defs/value' } },
    $defs: { value: { anyOf: [{ type: 'integer' }, { type: ['string', 'null'] }] } },
  };
  for (const value of [5, 'value', null]) assert.doesNotThrow(() => validateArguments({ value }, schema));
  assert.throws(() => validateArguments({ value: false }, schema), /anyOf matched 0 branches/);
  assert.throws(() => validateArguments({ value: 1.5 }, schema), /expected integer/);
});

test('oneOf requires exactly one matching branch, including const values', () => {
  assert.doesNotThrow(() => validateArguments('tap', { oneOf: [{ const: 'tap' }, { const: 'type' }] }));
  assert.throws(() => validateArguments('click', { oneOf: [{ const: 'tap' }, { const: 'type' }] }), /oneOf matched 0 branches/);
  assert.throws(() => validateArguments(1, { oneOf: [{ type: 'integer' }, { type: 'number' }] }), /oneOf matched 2 branches/);
});

test('object alternatives own their parameter names rather than the union wrapper', () => {
  const schema = { type: 'object', oneOf: [act.inputSchema, {
    type: 'object', required: ['checks'], properties: { checks: { type: 'array', items: { type: 'string' } } },
  }] };
  assert.doesNotThrow(() => validateArguments({ actions: [{ action: 'tap' }] }, schema));
  assert.doesNotThrow(() => validateArguments({ checks: ['no crash'] }, schema));
  assert.throws(() => validateArguments({ assertions: [] }, schema), /unknown property/);
});

test('explicit additionalProperties supports maps while omitted policy rejects unknown keys', () => {
  assert.doesNotThrow(() => validateArguments({ SECRET: 'value' }, { type: 'object', additionalProperties: { type: 'string' } }));
  assert.throws(() => validateArguments({ SECRET: 5 }, { type: 'object', additionalProperties: { type: 'string' } }), /arguments.SECRET: expected string/);
  assert.throws(() => validateArguments({ unknown: true }, { type: 'object' }), /arguments.unknown: unknown property/);
});

test('unsupported constraints fail even in unused union branches or tool schemas', () => {
  assert.throws(() => validateArguments('valid', { anyOf: [{ type: 'string' }, { type: 'string', minLength: 9 }] }), /unsupported schema keyword: minLength/);
  assert.throws(() => assertSupportedSchema({ $ref: 'https://example.invalid/schema' }), /unsupported schema \$ref/);
  assert.throws(() => assertSupportedSchema({ $ref: '#/$defs/missing' }), /unresolved schema \$ref/);
  assert.throws(() => validateSkillContracts([], [{ name: 'mdh_future', inputSchema: { format: 'date' } }]), /unsupported schema keyword: format/);
});
