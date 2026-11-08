#!/usr/bin/env node
// Read-only, dependency-free check; no Cargo invocation or generated files.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const iconRoot = path.join(root, 'assets/icons');
const errors = [];
const deferred = [];
let checked = 0;

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap(entry => {
      const file = path.join(directory, entry.name);
      return entry.isDirectory() ? walk(file) : [file];
    });
}

// Recognize Rust ordinary/byte/C strings and raw strings, ignoring comments
// (including nested block comments) and character literals, not lifetimes.
// This is a lexical check, not macro expansion or Rust expression evaluation.
function* literals(source) {
  const tokens = /\/\/[^\n]*|\/\*|(?:br|cr|r)(#{0,255})"|"(?:\\[\s\S]|[^"\\])*"|'(?:\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|[\s\S])|[^'\\\r\n])'/g;
  let match;
  while ((match = tokens.exec(source))) {
    const token = match[0];
    if (token.startsWith('//') || token.startsWith("'")) continue;
    if (token === '/*') {
      const comments = /\/\*|\*\//g;
      comments.lastIndex = tokens.lastIndex;
      let depth = 1;
      let comment;
      while (depth && (comment = comments.exec(source))) {
        depth += comment[0] === '/*' ? 1 : -1;
      }
      tokens.lastIndex = depth ? source.length : comments.lastIndex;
      continue;
    }
    let value;
    if (match[1] !== undefined) {
      const closing = `"${match[1]}`;
      const end = source.indexOf(closing, tokens.lastIndex);
      if (end === -1) break; // Syntax validation belongs to the Rust compiler.
      value = source.slice(tokens.lastIndex, end);
      tokens.lastIndex = end + closing.length;
    } else {
      value = token.slice(1, -1);
    }
    yield { value, index: match.index };
  }
}

function stat(file) {
  try {
    return statSync(file);
  } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'ENOTDIR') return undefined;
    throw error;
  }
}

function checkSource(file) {
  const source = readFileSync(file, 'utf8');
  const relative = path.relative(root, file);
  if (source.includes('\0')) {
    deferred.push(`${relative}: binary input not scanned`);
    return;
  }
  for (const { value, index } of literals(source)) {
    if (!value.includes('assets/icons')) continue;
    const location = `${relative}:${source.slice(0, index).split('\n').length}`;
    // Escapes, interpolation, glob patterns and embedded prose are not paths
    // we can resolve safely. Never treat their partial matches as real files.
    if (!/^(?:\.\.?\/)*assets\/icons(?:\/|$)/.test(value)
      || /[\\{}*?\[\]\r\n]/.test(value)) {
      deferred.push(`${location}: unresolved literal ${JSON.stringify(value)}`);
      continue;
    }
    const resolved = path.resolve(value.startsWith('.') ? path.dirname(file) : root, value);
    if (resolved !== iconRoot && !resolved.startsWith(`${iconRoot}${path.sep}`)) {
      errors.push(`${location}: icon path escapes assets/icons: ${value}`);
      continue;
    }
    const target = stat(resolved);
    if (target?.isDirectory() || value.endsWith('/') || !path.posix.extname(value)) {
      deferred.push(`${location}: directory/prefix ${JSON.stringify(value)}; dynamic children not checked`);
      continue;
    }
    if (!target?.isFile()) {
      errors.push(`${location}: missing icon file ${value}`);
    } else {
      checked += 1;
    }
  }
}

try {
  for (const entry of readdirSync(iconRoot, { withFileTypes: true })) {
    if (/\.svg$/i.test(entry.name)) {
      errors.push(`Root SVG leftover: assets/icons/${entry.name}`);
    }
  }
  const files = [...walk(path.join(root, 'src')), path.join(root, 'build.rs')];
  for (const file of files) checkSource(file);
  console.log(`Checked ${checked} literal icon file references across ${files.length} source files.`);
} catch (error) {
  errors.push(`Unable to complete asset check: ${error.message}`);
}

for (const message of deferred) console.warn(`REVIEW: ${message}`);
console.log('Scope: literal references in src/** and build.rs; no root SVGs.');
console.log('Limit: dynamic joins, format!/concat!, variable paths and escaped paths are not evaluated; review them manually.');
console.log('This lexical check does not prove runtime asset coverage or source/license rights.');
for (const message of errors) console.error(`ERROR: ${message}`);
if (errors.length) {
  console.error(`Asset check failed: ${errors.length} error(s).`);
  process.exitCode = 1;
} else {
  console.log(`Asset check passed (static references only; ${deferred.length} item(s) require review).`);
}
