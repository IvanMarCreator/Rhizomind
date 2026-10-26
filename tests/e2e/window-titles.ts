// Заголовки окон ОС у тестовой сборки (находка QA Н2, задача 2.11). WebDriver
// отдаёт заголовок страницы (`document.title`), а не заголовок окна, который
// видят панель задач и Alt+Tab, — поэтому заголовки читаются у ОС: в X11 —
// свойство _NET_WM_NAME (x11-titles.py), в Windows — имя окна верхнего уровня
// через UI Automation.
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { appPids } from './driver.ts';

function titlesOf(pid: number): string[] {
  const out =
    process.platform === 'win32'
      ? execFileSync(
          'powershell',
          [
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            [
              '[Console]::OutputEncoding = [Text.Encoding]::UTF8',
              'Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes',
              `$condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, ${pid})`,
              '[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition) | ForEach-Object { $_.Current.Name }',
            ].join('; '),
          ],
          { encoding: 'utf8' },
        )
      : execFileSync('python3', ['-I', path.join(import.meta.dirname, 'x11-titles.py'), String(pid)], {
          encoding: 'utf8',
        });
  return out
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== '');
}

/** Заголовки показанных окон всех процессов тестовой сборки, по алфавиту. */
export function appWindowTitles(): string[] {
  return appPids().flatMap(titlesOf).sort();
}
