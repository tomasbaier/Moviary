// Builds the Android APK (arm64 + armv7) and copies it to Release: `npm run android:build`.
//
// `tauri android build` links the compiled Rust library into the Android project with a
// symbolic link, which Windows only allows in Developer Mode. Without it this script lets
// Tauri compile the library, copies it into jniLibs itself and runs Gradle directly.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.join(__dirname, '..');
const androidDir = path.join(root, 'src-tauri/gen/android');
const TARGETS = [
  { target: 'aarch64', triple: 'aarch64-linux-android', abi: 'arm64-v8a', arch: 'arm64' },
  { target: 'armv7', triple: 'armv7-linux-androideabi', abi: 'armeabi-v7a', arch: 'arm' },
];
const LIB = 'libfilmoteka_lib.so';

function setupEnv() {
  const env = process.env;
  const studioJbr = 'C:\\Program Files\\Android\\Android Studio\\jbr';
  if (!env.JAVA_HOME && fs.existsSync(studioJbr)) env.JAVA_HOME = studioJbr;
  const sdk = env.ANDROID_HOME || path.join(env.LOCALAPPDATA || '', 'Android', 'Sdk');
  if (!env.ANDROID_HOME && fs.existsSync(sdk)) env.ANDROID_HOME = sdk;
  if (!env.NDK_HOME) {
    const ndkDir = path.join(sdk, 'ndk');
    const versions = fs.existsSync(ndkDir)
      ? fs.readdirSync(ndkDir).sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
      : [];
    if (versions.length) env.NDK_HOME = path.join(ndkDir, versions[versions.length - 1]);
  }
  for (const name of ['JAVA_HOME', 'ANDROID_HOME', 'NDK_HOME']) {
    if (!env[name]) {
      console.error(`Chybí ${name} – nainstalujte Android Studio a NDK (viz README).`);
      process.exit(1);
    }
  }
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, shell: true, encoding: 'utf8', ...options });
  return result;
}

function tauriBuild(targets) {
  // Output is shown live and also kept to recognise the symlink error.
  const result = run('npx', ['tauri', 'android', 'build', '--apk', '--ci', '--target', ...targets], {
    stdio: ['inherit', 'inherit', 'pipe'],
  });
  process.stderr.write(result.stderr || '');
  return result;
}

setupEnv();
const result = tauriBuild(TARGETS.map((t) => t.target));

if (result.status !== 0) {
  if (!/symbolic link/i.test(result.stderr || '')) process.exit(result.status || 1);
  console.log('\nSymbolické odkazy nejsou povolené (Režim pro vývojáře je vypnutý) – kopíruji knihovny ručně.\n');

  // Tauri stops after the first target, so compile each one on its own (the first one is
  // already up to date). Failing only at the symlink means the library was built.
  for (const t of TARGETS) {
    const lib = path.join(root, 'src-tauri/target', t.triple, 'release', LIB);
    const r = tauriBuild([t.target]);
    if ((r.status !== 0 && !/symbolic link/i.test(r.stderr || '')) || !fs.existsSync(lib)) {
      console.error(`Knihovnu pro ${t.target} se nepodařilo sestavit.`);
      process.exit(r.status || 1);
    }
    const dir = path.join(androidDir, 'app/src/main/jniLibs', t.abi);
    fs.mkdirSync(dir, { recursive: true });
    const dest = path.join(dir, LIB);
    fs.rmSync(dest, { force: true });
    fs.copyFileSync(lib, dest);
  }

  const gradle = run(
    `"${path.join(androidDir, process.platform === 'win32' ? 'gradlew.bat' : 'gradlew')}"`,
    [
      'assembleUniversalRelease',
      `-PabiList=${TARGETS.map((t) => t.abi).join(',')}`,
      `-ParchList=${TARGETS.map((t) => t.arch).join(',')}`,
      `-PtargetList=${TARGETS.map((t) => t.target).join(',')}`,
      '-x', 'rustBuildUniversalRelease',
      ...TARGETS.flatMap((t) => ['-x', `rustBuild${t.arch[0].toUpperCase()}${t.arch.slice(1)}Release`]),
    ],
    { cwd: androidDir, stdio: 'inherit' },
  );
  if (gradle.status !== 0) process.exit(gradle.status || 1);
}

const copy = run('node', [path.join(__dirname, 'release.js'), 'apk'], { stdio: 'inherit' });
process.exit(copy.status || 0);
