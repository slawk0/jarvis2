// The version lives in package.json. tauri.conf.json reads it from there;
// this script copies it into src-tauri/Cargo.toml and Cargo.lock.
//
//   pnpm version:sync            sync the current package.json version
//   pnpm version:sync 1.2.3      set a new version everywhere

import { readFileSync, writeFileSync } from 'node:fs';

const root = new URL('../', import.meta.url);
const read = (/** @type {string} */ path) => readFileSync(new URL(path, root), 'utf8');
const write = (/** @type {string} */ path, /** @type {string} */ text) =>
	writeFileSync(new URL(path, root), text);

const pkg = JSON.parse(read('package.json'));
const version = process.argv[2] ?? pkg.version;
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
	console.error(`Not a valid version: ${version}`);
	process.exit(1);
}

if (pkg.version !== version) {
	write('package.json', read('package.json').replace(/("version":\s*")[^"]+(")/, `$1${version}$2`));
}

// Only the [package] entry: the first `version = "…"` line in Cargo.toml.
write(
	'src-tauri/Cargo.toml',
	read('src-tauri/Cargo.toml').replace(/^version = "[^"]+"/m, `version = "${version}"`)
);

const name = /^name = "([^"]+)"/m.exec(read('src-tauri/Cargo.toml'))?.[1];
const lock = new RegExp(`(name = "${name}"\\r?\\nversion = ")[^"]+(")`);
write('src-tauri/Cargo.lock', read('src-tauri/Cargo.lock').replace(lock, `$1${version}$2`));

console.log(`Version ${version} written to package.json, Cargo.toml and Cargo.lock`);
