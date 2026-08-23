#!/usr/bin/env node
/**
 * Auto-detect GPU and run Tauri with appropriate features
 */

const { execFileSync, execSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

// Get the command (dev or build)
const command = process.argv[2];
if (!command || !['dev', 'build'].includes(command)) {
  console.error('Usage: node tauri-auto.js [dev|build]');
  process.exit(1);
}

// Detect GPU feature
let feature = '';

// Check for environment variable override first
if (process.env.TAURI_GPU_FEATURE) {
  feature = process.env.TAURI_GPU_FEATURE;
  console.log(`🔧 Using forced GPU feature from environment: ${feature}`);
} else {
  try {
    const result = execSync('node scripts/auto-detect-gpu.js', {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'inherit']
    });
    feature = result.trim();
  } catch (err) {
    // If detection fails, continue with no features
  }
}

console.log(''); // Empty line for spacing

// Build the llama-helper sidecar before handing over to Tauri.
//
// Tauri's `externalBin` expects src-tauri/binaries/llama-helper-<triple> to
// exist and aborts the build when it does not. This step used to live only in
// dev-gpu.sh, so a fresh clone built with the documented `pnpm run tauri:dev`
// failed with "resource path 'binaries/llama-helper-...' doesn't exist".
{
  const sidecarArgs = [path.join(__dirname, 'build-sidecar.js')];
  if (command === 'build') sidecarArgs.push('--release');

  try {
    execFileSync(process.execPath, sidecarArgs, {
      stdio: 'inherit',
      env: { ...process.env, TAURI_GPU_FEATURE: feature || '' },
    });
  } catch (err) {
    console.error('❌ Sidecar build failed; aborting before Tauri runs.');
    process.exit(err.status || 1);
  }
  console.log('');
}

// Platform-specific environment variables
const platform = os.platform();
const env = { ...process.env };

if (platform === 'linux' && feature === 'cuda') {
  console.log('🐧 Linux/CUDA detected: Setting CMAKE flags for NVIDIA GPU');
  env.CMAKE_CUDA_ARCHITECTURES = '75';
  env.CMAKE_CUDA_STANDARD = '17';
  env.CMAKE_POSITION_INDEPENDENT_CODE = 'ON';
}

// Optional data isolation for branch builds.
//
// Every build writes to the app data directory keyed by the bundle identifier,
// not by the branch, so a package built from a feature worktree shares one
// database with the main build. When the two branches' migrations differ, the
// build that runs second refuses to start: sqlx sees a migration in the
// database that its own binary has never heard of.
//
// Setting HALVERN_APP_SUFFIX overrides the identifier for this build, giving
// it its own database, settings and logs. It is opt-in on purpose — an
// automatic switch would silently hand a feature build an empty database,
// which is useless for testing anything against real recordings.
let configOverridePath = null;
const appSuffix = (process.env.HALVERN_APP_SUFFIX || '').trim();
if (appSuffix) {
  if (!/^[A-Za-z0-9-]+$/.test(appSuffix)) {
    console.error(
      `❌ HALVERN_APP_SUFFIX must be letters, numbers or hyphens (got "${appSuffix}").`
    );
    console.error('   It becomes part of a bundle identifier and a directory name.');
    process.exit(1);
  }

  const baseConfigPath = path.join(__dirname, '..', 'src-tauri', 'tauri.conf.json');
  const baseConfig = JSON.parse(fs.readFileSync(baseConfigPath, 'utf8'));
  const identifier = `${baseConfig.identifier}.${appSuffix}`;
  const productName = `${baseConfig.productName}-${appSuffix}`;

  configOverridePath = path.join(
    __dirname,
    '..',
    'src-tauri',
    '.tauri.suffix.conf.json'
  );
  fs.writeFileSync(
    configOverridePath,
    JSON.stringify({ identifier, productName }, null, 2)
  );

  console.log(`🧪 Isolated build: ${identifier}`);
  console.log(`   Data:  ~/Library/Application Support/${identifier}/`);
  console.log(`   Logs:  ~/Library/Logs/${identifier}/`);
  console.log('   macOS treats this as a separate app, so microphone and screen');
  console.log('   recording permissions are requested again on first run.');
  console.log('');
}

// Build the tauri command. `--config` is a Tauri argument, so it has to sit
// before the `--` that forwards the rest to cargo.
let tauriCmd = `tauri ${command}`;
if (configOverridePath) {
  tauriCmd += ` --config ${JSON.stringify(configOverridePath)}`;
}
if (feature && feature !== 'none') {
  tauriCmd += ` -- --features ${feature}`;
  console.log(`🚀 Running: tauri ${command} with features: ${feature}`);
} else {
  console.log(`🚀 Running: tauri ${command} (CPU-only mode)`);
}
console.log('');

// Execute the command
try {
  execSync(tauriCmd, { stdio: 'inherit', env });
} catch (err) {
  process.exit(err.status || 1);
}
