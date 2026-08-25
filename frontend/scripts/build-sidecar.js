#!/usr/bin/env node
/**
 * Builds the `llama-helper` sidecar and installs it where Tauri expects it.
 *
 * Tauri resolves `externalBin: ["binaries/llama-helper"]` to
 * `src-tauri/binaries/llama-helper-<target-triple>`. Without that file the build
 * fails with `resource path 'binaries/llama-helper-...' doesn't exist`.
 *
 * This logic used to live only in `dev-gpu.sh`, so a fresh clone built with the
 * documented `pnpm run tauri:dev` never produced the sidecar. Extracted into its
 * own script so it can be run and verified on its own:
 *
 *   node scripts/build-sidecar.js            # debug
 *   node scripts/build-sidecar.js --release  # release
 *
 * Set CARGO_NET_OFFLINE=true to build without network access.
 */

// execFileSync only: no shell is spawned, so nothing here can be shell-injected.
const { execFileSync } = require('child_process');
const path = require('path');
const fs = require('fs');

const FRONTEND_DIR = path.resolve(__dirname, '..');
const WORKSPACE_ROOT = path.resolve(FRONTEND_DIR, '..');
const HELPER_DIR = path.join(WORKSPACE_ROOT, 'llama-helper');
const BINARIES_DIR = path.join(FRONTEND_DIR, 'src-tauri', 'binaries');

const isRelease = process.argv.includes('--release');
const profileDir = isRelease ? 'release' : 'debug';

/**
 * Picks the GPU feature for llama-helper.
 *
 * llama-cpp-2 has no CoreML backend, so the `coreml` feature that GPU detection
 * returns for Apple Silicon is mapped to `metal`. Mirrors dev-gpu.sh:105-108.
 */
function resolveFeature() {
  let feature = process.env.TAURI_GPU_FEATURE || '';

  if (!feature) {
    try {
      feature = execFileSync(process.execPath, ['scripts/auto-detect-gpu.js'], {
        cwd: FRONTEND_DIR,
        encoding: 'utf8',
        stdio: ['pipe', 'pipe', 'inherit'],
      }).trim();
    } catch {
      // Detection is best-effort; fall through to a CPU build.
      feature = '';
    }
  }

  if (feature === 'coreml') {
    console.log("   llama-cpp-2 has no CoreML backend, using Metal instead");
    feature = 'metal';
  }

  return feature && feature !== 'none' ? feature : '';
}

function targetTriple() {
  const out = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const line = out.split('\n').find((l) => l.startsWith('host:'));
  if (!line) {
    throw new Error('Could not read the target triple from `rustc -vV`');
  }
  return line.split(':')[1].trim();
}

function main() {
  if (!fs.existsSync(HELPER_DIR)) {
    console.error(`❌ No llama-helper directory at: ${HELPER_DIR}`);
    process.exit(1);
  }

  const feature = resolveFeature();
  const args = ['build'];
  if (isRelease) args.push('--release');
  if (feature) args.push('--features', feature);

  console.log(`🦙 Building llama-helper (${profileDir}), features: ${feature || 'none'}`);

  try {
    execFileSync('cargo', args, { cwd: HELPER_DIR, stdio: 'inherit' });
  } catch (err) {
    console.error('❌ Building llama-helper failed');
    process.exit(err.status || 1);
  }

  const triple = targetTriple();
  const isWindows = process.platform === 'win32';
  const baseName = isWindows ? 'llama-helper.exe' : 'llama-helper';
  const sidecarName = isWindows
    ? `llama-helper-${triple}.exe`
    : `llama-helper-${triple}`;

  const srcPath = path.join(WORKSPACE_ROOT, 'target', profileDir, baseName);
  if (!fs.existsSync(srcPath)) {
    console.error(`❌ No built binary at: ${srcPath}`);
    process.exit(1);
  }

  fs.mkdirSync(BINARIES_DIR, { recursive: true });

  // Drop stale sidecars so a changed target triple cannot leave two behind.
  for (const entry of fs.readdirSync(BINARIES_DIR)) {
    if (entry.startsWith('llama-helper')) {
      fs.rmSync(path.join(BINARIES_DIR, entry), { force: true });
    }
  }

  const destPath = path.join(BINARIES_DIR, sidecarName);
  fs.copyFileSync(srcPath, destPath);
  fs.chmodSync(destPath, 0o755);

  console.log(`✅ Sidecar zainstalowany: ${destPath}`);
}

main();
