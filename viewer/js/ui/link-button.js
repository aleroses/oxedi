// A button that jumps to a segment. The caller sets the text, the title and data-segment.

import { el } from '../dom.js';

export function linkButton() {
  const button = el('button', 'link');
  button.type = 'button';
  return button;
}
