#!/usr/bin/env node
// Local publication guard. Print categories and locations, never matched values.
// This targeted check is not a comprehensive secret scanner.
const fs = require('node:fs');
const cp = require('node:child_process');

function git(args, input) {
  const result = cp.spawnSync('git', args, {
    input, windowsHide: true, maxBuffer: 128 * 1024 * 1024,
  });
  if (result.status !== 0) throw new Error(`git ${args[0]} failed`);
  return result.stdout;
}

const rules = [
  ['private key', /-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY(?: BLOCK)?-----/],
  ['provider token', /\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}|AIza[0-9A-Za-z_-]{30,}|(?:AKIA|ASIA)[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{15,}|glpat-[A-Za-z0-9_-]{15,})\b/],
  ['personal filesystem path', /(?:[A-Z]:[\\/](?:Users|Documents and Settings)[\\/]|\/(?:home|Users)\/)[^\s"'<>]+/i],
  ['AI conversation URL', /https?:\/\/(?:claude\.(?:ai|com)|chatgpt\.com|chat\.openai\.com)\/(?:code\/|chat\/|share\/|c\/|s\/)[^\s<>)"']+/i],
  ['credential-bearing URL', /\b(?:https?|ftp|postgres(?:ql)?|mysql|mongodb(?:\+srv)?):\/\/[^\s/:@]+:[^\s/@]+@[^\s/]+/i],
];
const privatePath = /(?:^|\/)(?:\.claude|\.codex|\.agents|assets|artifacts|playdata|fixtures|workspace|outputs)(?:\/|$)|(?:^|\/)(?:CLAUDE\.md|AGENTS\.md|GOAL\.md|SESSION-NOTES\.md|spec\.txt|dialogue\.txt)$|\.(?:jar|jad|class|7z|zip|pem|key|p12|pfx|log|db|sqlite)$|\.javap\./i;
const screenshots = new Set(['screenshots/gameplay.png', 'screenshots/kvatch-full-level.png']);
let failures = 0;

function fail(location, category) {
  console.error(`${location}: ${category}`);
  failures++;
}
function checkText(bytes, location) {
  const text = bytes.toString('utf8');
  for (const [category, pattern] of rules) {
    if (pattern.test(text)) fail(location, category);
  }
}
function checkIdentity(identity, location) {
  const email = identity.match(/<([^>]+)>/)?.[1];
  if (!email || !(/@users\.noreply\.github\.com$/i.test(email) || /^noreply@(?:github|anthropic)\.com$/i.test(email))) {
    fail(location, 'use a GitHub noreply author/committer email');
  }
}
function checkPath(name) {
  const basename = name.split('/').pop();
  if (privatePath.test(name) || (basename.startsWith('.env') && basename !== '.env.example')) {
    fail(name, 'private development material or credential file');
  }
}

try {
  const args = process.argv.slice(2);
  if (args[0] === '--commit-message') {
    if (args.length !== 2) throw new Error('Expected a commit message filename');
    checkText(fs.readFileSync(args[1]), 'commit message');
    checkIdentity(git(['var', 'GIT_AUTHOR_IDENT']).toString(), 'author');
    checkIdentity(git(['var', 'GIT_COMMITTER_IDENT']).toString(), 'committer');
  } else if (args.length === 0 || (args.length === 1 && ['--staged', '--history'].includes(args[0]))) {
    const staged = git(['ls-files', '--stage', '-z']).toString().split('\0').filter(Boolean);
    const blobs = new Map();
    for (const entry of staged) {
      const [header, name] = entry.split('\t');
      const [mode, oid, stage] = header.split(' ');
      checkPath(name);
      if (stage !== '0') fail(name, 'unresolved merge');
      if (mode === '120000' || mode === '160000') fail(name, 'symlink or submodule requires explicit publication review');
      else blobs.set(oid, name);
    }
    if (args[0] === '--history') {
      const history = git(['rev-list', '--objects', '--all']).toString().trim().split('\n');
      const bytes = git(['cat-file', '--batch'], history.map(line => line.split(' ')[0]).join('\n') + '\n');
      const names = new Map(history.map(line => [line.split(' ')[0], line.slice(41)]));
      let offset = 0;
      while (offset < bytes.length) {
        const end = bytes.indexOf(10, offset);
        const [oid, type, size] = bytes.subarray(offset, end).toString().split(' ');
        const data = bytes.subarray(end + 1, end + 1 + Number(size));
        offset = end + 1 + Number(size) + 1;
        if (type === 'commit' || type === 'tag') {
          checkText(data, `${type} ${oid.slice(0, 12)}`);
          for (const line of data.toString().split('\n')) {
            if (/^(?:author|committer|tagger) /.test(line)) checkIdentity(line, `${type} ${oid.slice(0, 12)}`);
          }
        } else if (type === 'blob') {
          const name = names.get(oid) || oid.slice(0, 12);
          checkPath(name);
          checkText(data, name);
          if (data.subarray(0, 8192).includes(0) && !screenshots.has(name)) fail(name, 'unreviewed binary file');
        }
      }
    }
    for (const [oid, name] of blobs) {
      const data = git(['cat-file', 'blob', oid]);
      checkText(data, name);
      if (data.subarray(0, 8192).includes(0) && !screenshots.has(name)) fail(name, 'unreviewed binary file');
    }
    if (args[0] === '--staged') {
      checkIdentity(git(['var', 'GIT_AUTHOR_IDENT']).toString(), 'author');
      checkIdentity(git(['var', 'GIT_COMMITTER_IDENT']).toString(), 'committer');
    }
  } else {
    throw new Error('Usage: node tools/check-publication.cjs [--staged|--history|--commit-message FILE]');
  }
} catch (error) {
  fail('publication check', error.message);
}
if (failures) {
  console.error('Publication check failed. Review the listed locations locally.');
  process.exit(1);
}
console.log('Publication check passed. Manually review screenshots and newly added content too.');
