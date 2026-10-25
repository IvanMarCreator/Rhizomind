// Тестовая сборка для сценарных тестов (npm run build:e2e): функция cargo
// `webdriver` и отдельная папка сборки src-tauri/target-e2e, чтобы тестовый
// исполняемый файл никогда не оказался там, откуда берут выпускной
// (src-tauri/target/release — package:portable, тест следов).
import { spawnSync } from 'node:child_process';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..', '..');
const tauri = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

const result = spawnSync(process.execPath, [tauri, 'build', '--no-bundle', '--features', 'webdriver'], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, CARGO_TARGET_DIR: path.join(root, 'src-tauri', 'target-e2e') },
});
process.exit(result.status ?? 1);
