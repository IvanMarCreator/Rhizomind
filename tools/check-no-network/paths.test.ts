import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { scanPaths } from './scan.ts';

function tree(files: Record<string, string>): string {
  const root = mkdtempSync(path.join(tmpdir(), 'check-no-network-'));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    writeFileSync(path.join(root, name), text);
  }
  return root;
}

describe('scanPaths', () => {
  it('обходит папки и проверяет исходники и сборку, пропуская прочие файлы и модульные тесты', async () => {
    const root = tree({
      'src/main.ts': 'fetch(u);\n',
      'src/ui/App.svelte': '<script>\nnew WebSocket(u)\n</script>\n',
      'src/readme.md': 'fetch(u)',
      'src/api/core.test.ts': 'vi.stubGlobal("window", globalThis); fetch(u);',
      'dist/assets/index-abc.js': 'const ok = 1;\n',
      'dist/index.html': '<script src="//cdn.test/x.js"></script>',
    });
    const found = await scanPaths([path.join(root, 'src'), path.join(root, 'dist')], root);
    expect(found).toEqual([
      { file: 'dist/index.html', line: 1, what: 'внешний адрес //cdn.test/x.js', severity: 'network' },
      { file: 'src/main.ts', line: 1, what: 'fetch', severity: 'network' },
      { file: 'src/ui/App.svelte', line: 2, what: 'WebSocket', severity: 'network' },
    ]);
  });

  it('файл, который не удалось разобрать, — подозрительно (проверка не пропускает его молча)', async () => {
    const root = tree({ 'src/bad.ts': 'const = ;' });
    const found = await scanPaths([path.join(root, 'src')], root);
    expect(found).toHaveLength(1);
    expect(found[0]).toMatchObject({ file: 'src/bad.ts', severity: 'suspicious' });
    expect(found[0]?.what).toMatch(/^не удалось разобрать/);
  });

  it('несуществующая папка — ошибка, а не «сети нет»', async () => {
    await expect(scanPaths(['/nonexistent-dir-for-test'], '/')).rejects.toThrow();
  });
});
