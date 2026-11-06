// The Diagnostics tab: findings grouped by level, each linked to its segment.

import { el } from './dom.js';
import { diagnosticRow } from './ui/diagnostic-row.js';

const LEVELS = ['SNIP 1', 'SNIP 2', 'SNIP 3'];
const PAGE = 200;

/**
 * @param {HTMLElement} root
 * @param {{onJump: (segment: number, element?: number, component?: number) => void}} options
 */
export function createDiagnostics(root, { onJump }) {
  let diagnostics = [];
  let level = 'all';
  const tools = el('div', 'diag-tools');
  tools.setAttribute('role', 'group');
  tools.setAttribute('aria-label', 'Filter by level');
  const body = el('div');
  root.append(tools, body);

  function renderList() {
    body.textContent = '';
    const shown = diagnostics.filter((d) => level === 'all' || d.level === level);
    if (diagnostics.length === 0) {
      body.append(el('p', 'muted', 'No findings: the file passes every check this spec runs.'));
      return;
    }
    if (shown.length === 0) {
      body.append(el('p', 'muted', 'No findings at this level.'));
      return;
    }
    const list = el('ul', 'diag-list');
    body.append(list);
    let next = 0;
    const more = el('button', null);
    more.type = 'button';
    const fill = () => {
      for (const d of shown.slice(next, next + PAGE)) list.append(diagnosticRow(d, onJump));
      next = Math.min(shown.length, next + PAGE);
      more.hidden = next >= shown.length;
      more.textContent = `Show more (${shown.length - next} left)`;
    };
    more.addEventListener('click', fill);
    body.append(more);
    fill();
  }

  function renderTools() {
    tools.textContent = '';
    const options = [['all', 'All', diagnostics.length]];
    for (const l of LEVELS) options.push([l, l, diagnostics.filter((d) => d.level === l).length]);
    for (const [value, label, count] of options) {
      const button = el('button', null, `${label} (${count})`);
      button.type = 'button';
      button.setAttribute('aria-pressed', String(level === value));
      button.addEventListener('click', () => {
        level = value;
        renderTools();
        renderList();
      });
      tools.append(button);
    }
  }

  return {
    show({ view }) {
      diagnostics = view.diagnostics;
      level = 'all';
      renderTools();
      renderList();
    },
    state() {
      return { level };
    },
    restore(saved) {
      if (!saved || (saved.level !== 'all' && !LEVELS.includes(saved.level))) return;
      level = saved.level;
      renderTools();
      renderList();
    },
    clear() {
      diagnostics = [];
      tools.textContent = '';
      body.textContent = '';
    },
  };
}
