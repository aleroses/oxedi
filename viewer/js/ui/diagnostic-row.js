// One diagnostic finding: its level, the jump to its segment, the text and the loop path.

import { el } from '../dom.js';

function pathText(path) {
  return path.map(([name, ordinal]) => `${name} #${ordinal}`).join(' › ');
}

/**
 * @param {object} finding
 * @param {(segment: number, element?: number, component?: number) => void} onJump
 */
export function diagnosticRow(finding, onJump) {
  const li = el('li', 'diag');
  const head = el('div');
  head.append(el('span', 'level', finding.level));
  if (finding.segment !== null) {
    const jump = el('button', 'jump', `segment ${finding.segment}`);
    jump.type = 'button';
    jump.addEventListener('click', () => onJump(finding.segment, finding.element ?? undefined, finding.component ?? undefined));
    head.append(jump);
  } else {
    head.append(el('span', 'muted', 'end of file'));
  }
  li.append(head, el('div', 'text', finding.text));
  if (finding.path.length) li.append(el('div', 'path', pathText(finding.path)));
  return li;
}
