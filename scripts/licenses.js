// Writes src/third-party-licenses.txt: the licenses of everything shipped inside the app
// (Rust crates of the Windows and Android builds, Android libraries). The app shows it in
// "O aplikaci" and the release script puts a copy next to the exe and apk.
// Run automatically by `npm run build` and `npm run android:build`: `node scripts/licenses.js`.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const root = path.join(__dirname, '..');
const output = path.join(root, 'src', 'third-party-licenses.txt');
const { version } = require(path.join(root, 'package.json'));

// Platforms the app is built for (the Android ABIs share their dependencies).
const PLATFORMS = ['x86_64-pc-windows-msvc', 'aarch64-linux-android'];

// Libraries the Android app adds through Gradle (src-tauri/gen/android/app/build.gradle.kts)
// and the Tauri Android runtime; all Apache-2.0.
const ANDROID_LIBRARIES = [
  'AndroidX (appcompat, activity-ktx, webkit, lifecycle-process) – The Android Open Source Project',
  'Material Components for Android – Google',
  'Kotlin standard library – JetBrains',
  'Tauri Android runtime – The Tauri Programme within The Commons Conservancy',
];

const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$/i;

const metadata = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--manifest-path', path.join(root, 'src-tauri', 'Cargo.toml'),
      ...PLATFORMS.flatMap((p) => ['--filter-platform', p])],
    { maxBuffer: 256 * 1024 * 1024, encoding: 'utf8' },
  ),
);

// Everything reachable from the app except dev-dependencies.
const packages = new Map(metadata.packages.map((p) => [p.id, p]));
const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
const rootId = metadata.resolve.root;
const used = new Set();
const queue = [rootId];
while (queue.length) {
  const id = queue.pop();
  if (used.has(id)) continue;
  used.add(id);
  for (const dep of nodes.get(id)?.deps ?? []) {
    if (dep.dep_kinds.some((k) => k.kind !== 'dev')) queue.push(dep.pkg);
  }
}
used.delete(rootId);

function licenseFiles(dir) {
  const files = [];
  const walk = (folder, depth) => {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      const full = path.join(folder, entry.name);
      if (entry.isFile() && LICENSE_FILE.test(entry.name)) files.push(full);
      else if (entry.isDirectory() && depth === 0 && /^licen[cs]es?$/i.test(entry.name)) walk(full, 1);
    }
  };
  walk(dir, 0);
  return files.sort();
}

const normalize = (text) => text.replace(/\r\n?/g, '\n').replace(/[ \t]+$/gm, '').trim();

// License text -> packages using it, so the same Apache-2.0 text appears once.
const texts = new Map();
const list = [];
const withoutFiles = [];
for (const id of used) {
  const pkg = packages.get(id);
  const name = `${pkg.name} ${pkg.version}`;
  list.push({ name, license: pkg.license || (pkg.license_file ? 'see license file' : 'unknown'), repository: pkg.repository || '' });
  const files = licenseFiles(path.dirname(pkg.manifest_path));
  if (!files.length) withoutFiles.push({ name, license: pkg.license || 'unknown', authors: pkg.authors });
  for (const file of files) {
    const text = normalize(fs.readFileSync(file, 'utf8'));
    if (!text) continue;
    if (!texts.has(text)) texts.set(text, []);
    texts.get(text).push(name);
  }
}
list.sort((a, b) => a.name.localeCompare(b.name));

const line = '='.repeat(78);
let out = `Filmotéka ${version} – third-party software
${line}

Filmotéka contains the open-source software listed below. Each component is
used under its own license; the license texts follow the list. Filmotéka
itself is MIT licensed (see below). Components under the MPL-2.0 are used
unmodified; their source code is available from the repositories listed.

Not included, only used:
- VLC media player (https://www.videolan.org/) is installed separately by
  the user and started as a separate program. VLC is a trademark of VideoLAN.
- Movie and series information and images come from TMDB
  (https://www.themoviedb.org/) through the TMDB API, with the user's own
  API key. This application uses TMDB and the TMDB APIs but is not endorsed,
  certified, or otherwise approved by TMDB. The TMDB logo is a trademark of
  TMDB, shown as TMDB's terms require.
- Microsoft Edge WebView2 (Windows) and Android System WebView display the
  app; they are part of the operating system.

${line}
Filmotéka
${line}

${normalize(fs.readFileSync(path.join(root, 'LICENSE'), 'utf8'))}

${line}
Android libraries (Apache License 2.0, text below)
${line}

${ANDROID_LIBRARIES.map((l) => `- ${l}`).join('\n')}

${line}
Rust crates (${list.length})
${line}

${list.map((p) => `- ${p.name} – ${p.license}${p.repository ? ` – ${p.repository}` : ''}`).join('\n')}
`;

if (withoutFiles.length) {
  out += `
${line}
Crates that ship no license file (their license is the standard text of the
license named; the copyright holders are their authors)
${line}

${withoutFiles.map((p) => `- ${p.name} – ${p.license}${p.authors?.length ? ` – Copyright ${p.authors.join(', ')}` : ''}`).join('\n')}
`;
}

for (const [text, users] of texts) {
  out += `
${line}
Used by: ${users.sort().join(', ')}
${line}

${text}
`;
}

fs.writeFileSync(output, out.replace(/\n/g, '\r\n'));
const kb = Math.round(fs.statSync(output).size / 1024);
console.log(`Licence ${list.length} knihoven zapsány do ${path.relative(root, output)} (${kb} kB)`);
