#!/usr/bin/env node
/**
 * Copy the main build's app data into an isolated branch build's directory.
 *
 * HALVERN_APP_SUFFIX builds get their own empty database, which is the point —
 * they cannot corrupt or migration-lock the real one. But an empty database is
 * useless for testing anything against real recordings, so this seeds the
 * isolated directory with a copy of the real one.
 *
 * It is a copy, not a link: the branch build then migrates and writes its own
 * data, and the real database stays untouched whatever the branch does to it.
 *
 * Usage:
 *   node scripts/seed-isolated-data.js <suffix> [--force]
 *
 * Example:
 *   node scripts/seed-isolated-data.js porcja8
 */

const fs = require('fs');
const path = require('path');
const os = require('os');

const [, , suffix, ...flags] = process.argv;
const force = flags.includes('--force');

if (!suffix) {
  console.error('Usage: node scripts/seed-isolated-data.js <suffix> [--force]');
  console.error('The suffix must match the HALVERN_APP_SUFFIX used for the build.');
  process.exit(1);
}

if (!/^[A-Za-z0-9-]+$/.test(suffix)) {
  console.error(`Suffix must be letters, numbers or hyphens (got "${suffix}").`);
  process.exit(1);
}

if (os.platform() !== 'darwin') {
  console.error('This helper only knows the macOS data directory layout.');
  process.exit(1);
}

const baseConfig = JSON.parse(
  fs.readFileSync(path.join(__dirname, '..', 'src-tauri', 'tauri.conf.json'), 'utf8')
);
const appSupport = path.join(os.homedir(), 'Library', 'Application Support');
const source = path.join(appSupport, baseConfig.identifier);
const target = path.join(appSupport, `${baseConfig.identifier}.${suffix}`);

if (!fs.existsSync(source)) {
  console.error(`No data to copy: ${source} does not exist.`);
  process.exit(1);
}

if (fs.existsSync(target) && !force) {
  console.error(`${target} already exists.`);
  console.error('Pass --force to replace it. Anything recorded in the isolated');
  console.error('build since the last seed will be lost.');
  process.exit(1);
}

if (fs.existsSync(target)) {
  fs.rmSync(target, { recursive: true, force: true });
}

// Models are large and identical between builds; skipping them keeps this
// from copying several gigabytes. The isolated build reads them from its own
// directory, so they are re-downloaded — or symlink them manually if that
// matters more than disk space.
fs.cpSync(source, target, {
  recursive: true,
  filter: (src) => path.basename(src) !== 'models',
});

console.log(`Copied ${source}`);
console.log(`     -> ${target}`);
console.log('');
console.log('Models were skipped (large, and the isolated build keeps its own).');
console.log('The real database is untouched; this copy is now independent.');
