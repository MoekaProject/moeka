#!/usr/bin/env node
// Export current reviewed working files for validation, never Git history.
// This is not a credential scrubber or authorization to make the result public.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = fs.realpathSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'));
const destinationArg = process.argv[2];
if (!destinationArg || process.argv.length !== 3) {
  throw new Error('Usage: node scripts/export-source-snapshot.mjs <new-directory-outside-repository>');
}
const destination = path.resolve(destinationArg);
const parent = fs.realpathSync(path.dirname(destination));
const resolvedDestination = path.join(parent, path.basename(destination));
const withinRoot = path.relative(root, resolvedDestination);
if (withinRoot === '' || (!withinRoot.startsWith(`..${path.sep}`) && withinRoot !== '..' && !path.isAbsolute(withinRoot))) {
  throw new Error('The snapshot must be outside the working repository.');
}
if (fs.existsSync(resolvedDestination)) {
  throw new Error('Destination must not exist; no existing files will be replaced.');
}
const listed = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
  cwd: root,
  maxBuffer: 16 * 1024 * 1024,
}).toString().split('\0').filter(Boolean);
const forbidden = /(^|\/)(\.git|\.venv|\.kilo|\.cargo-home|target|dist|audit-reports)(\/|$)|(^|\/)\.env($|\.)|\.(p12|p8|pfx|pem|key|provisionprofile|mobileprovision)$/i;
const files = [];
for (const relative of [...new Set(listed)].sort()) {
  if (path.isAbsolute(relative) || relative.split('/').includes('..')) {
    throw new Error('Unexpected path outside repository.');
  }
  const source = path.join(root, relative);
  let stat;
  try {
    stat = fs.lstatSync(source);
  } catch (error) {
    if (error.code === 'ENOENT') continue; // Intentional working-tree deletion.
    throw error;
  }
  if (relative === 'config.yaml' || forbidden.test(relative)) {
    throw new Error(`Review required: local-state or credential-like path selected: ${relative}`);
  }
  if (!stat.isFile() || stat.isSymbolicLink()) {
    throw new Error(`Only regular files can be exported: ${relative}`);
  }
  const realRelative = path.relative(root, fs.realpathSync(source));
  if (realRelative.startsWith(`..${path.sep}`) || path.isAbsolute(realRelative)) {
    throw new Error(`File resolves outside repository: ${relative}`);
  }
  files.push({ relative, source, mode: stat.mode & 0o777 });
}
fs.mkdirSync(resolvedDestination);
for (const { relative, source, mode } of files) {
  const target = path.join(resolvedDestination, relative);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(source, target, fs.constants.COPYFILE_EXCL);
  fs.chmodSync(target, mode);
}
console.log(`Exported ${files.length} working files without Git metadata to ${resolvedDestination}`);
console.log('Includes non-ignored untracked files. Review, scan and test this exact snapshot before use.');
console.log('No commit, push, ref deletion or visibility change was performed.');
