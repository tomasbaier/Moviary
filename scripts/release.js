// Copies the built app into the Release folder: `node scripts/release.js exe|apk`.
// Run automatically by `npm run build` and `npm run android:build`.

const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const { version } = require(path.join(root, 'package.json'));
const releaseDir = path.join(root, 'Release');

const apkDir = path.join(root, 'src-tauri/gen/android/app/build/outputs/apk/universal/release');
const ARTIFACTS = {
  exe: {
    from: [path.join(root, 'src-tauri/target/release/moviary.exe')],
    to: `Moviary-${version}-windows.exe`,
  },
  apk: {
    // Signed when src-tauri/gen/android/keystore.properties exists, unsigned otherwise.
    from: [path.join(apkDir, 'app-universal-release.apk'), path.join(apkDir, 'app-universal-release-unsigned.apk')],
    to: `Moviary-${version}-android.apk`,
  },
};

const kind = process.argv[2];
const artifact = ARTIFACTS[kind];
if (!artifact) {
  console.error('Použití: node scripts/release.js exe|apk');
  process.exit(1);
}
const source = artifact.from.find((p) => fs.existsSync(p));
if (!source) {
  console.error(`Sestavený soubor nebyl nalezen: ${artifact.from.join(', ')}`);
  process.exit(1);
}
if (source.endsWith('-unsigned.apk')) {
  console.warn('Pozor: APK není podepsané a na telefon nepůjde nainstalovat (chybí keystore.properties).');
}
fs.mkdirSync(releaseDir, { recursive: true });
const target = path.join(releaseDir, artifact.to);
fs.copyFileSync(source, target);
console.log(`Zkopírováno do ${path.relative(root, target)}`);

// Licenses of the bundled libraries belong next to the downloads (scripts/licenses.js).
const licenses = path.join(root, 'src', 'third-party-licenses.txt');
if (fs.existsSync(licenses)) {
  fs.copyFileSync(licenses, path.join(releaseDir, `Moviary-${version}-third-party-licenses.txt`));
}
