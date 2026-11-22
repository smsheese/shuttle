import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/whatsappMarkup.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { markupRuns, whatsappBlocks } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

/** Flat "text|style,style" form, so an assertion reads as what the user sees. */
const flat = (runs) => runs.map((run) => (run.styles.length ? `${run.text}|${run.styles.join(',')}` : run.text)).join('');

test('the four inline markers each style their own text', () => {
  assert.equal(flat(markupRuns('*bold*')), 'bold|bold');
  assert.equal(flat(markupRuns('_italic_')), 'italic|italic');
  assert.equal(flat(markupRuns('~strike~')), 'strike|strike');
  assert.equal(flat(markupRuns('`mono`')), 'mono|mono');
});

test('surrounding text is untouched and markers are not shown', () => {
  assert.equal(flat(markupRuns('see *this* now')), 'see this|bold now');
  assert.equal(flat(markupRuns('*bold* and _italic_')), 'bold|bold and italic|italic');
});

test('markup nests', () => {
  assert.equal(flat(markupRuns('*bold _and italic_*')), 'bold |bold' + 'and italic|bold,italic');
  assert.equal(flat(markupRuns('_a ~b~ c_')), 'a |italic' + 'b|italic,strike' + ' c|italic');
});

test('a mono run keeps its content literal', () => {
  assert.equal(flat(markupRuns('`*not bold*`')), '*not bold*|mono');
});

test('a lone marker stays literal', () => {
  assert.equal(flat(markupRuns('2 * 3 = 6')), '2 * 3 = 6');
  assert.equal(flat(markupRuns('*unclosed')), '*unclosed');
  assert.equal(flat(markupRuns('a ` b')), 'a ` b');
});

test('words with underscores or digits around a marker stay literal', () => {
  assert.equal(flat(markupRuns('snake_case_name')), 'snake_case_name');
  assert.equal(flat(markupRuns('file_name_here and more')), 'file_name_here and more');
  assert.equal(flat(markupRuns('2*3*4')), '2*3*4');
  assert.equal(flat(markupRuns('some~thing~else')), 'some~thing~else');
});

test('a marker followed by a space does not open a run', () => {
  assert.equal(flat(markupRuns('* not bold *')), '* not bold *');
  assert.equal(flat(markupRuns('_ not italic _')), '_ not italic _');
});

test('a plain body is one text block, newlines kept', () => {
  const blocks = whatsappBlocks('line one\nline two');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, 'text');
  assert.equal(blocks[0].runs.map((r) => r.text).join(''), 'line one\nline two');
});

test('bullet, ordered and quote lines become their own blocks', () => {
  const blocks = whatsappBlocks('- one\n- two\n1. first\n> quoted\nplain');
  assert.deepEqual(blocks.map((b) => b.kind), ['bullet', 'bullet', 'ordered', 'quote', 'text']);
  assert.equal(blocks[0].runs[0].text, 'one');
  assert.equal(blocks[2].marker, '1.');
  assert.equal(blocks[3].runs[0].text, 'quoted');
});

test('an asterisk bullet wins over bold', () => {
  const blocks = whatsappBlocks('* one\n* two');
  assert.deepEqual(blocks.map((b) => b.kind), ['bullet', 'bullet']);
});

test('a fenced block is literal mono and closes on the next fence', () => {
  const blocks = whatsappBlocks('before\n```\n*a* _b_\nline 2\n```\nafter');
  assert.deepEqual(blocks.map((b) => b.kind), ['text', 'code', 'text']);
  assert.equal(blocks[1].text, '*a* _b_\nline 2');
});

test('an unterminated fence still renders its content', () => {
  const blocks = whatsappBlocks('```\ncode');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, 'code');
  assert.equal(blocks[0].text, 'code');
});

test('a fence written inside one line is inline mono', () => {
  const blocks = whatsappBlocks('try ```npm test``` now');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, 'text');
  assert.equal(flat(blocks[0].runs), 'try npm test|mono now');
});

test('blank lines do not create empty blocks', () => {
  assert.deepEqual(whatsappBlocks('\n\n'), []);
});
