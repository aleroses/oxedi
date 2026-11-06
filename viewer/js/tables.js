// The Tables tab: a table selector, a text filter, a virtualized grid and CSV export.

import { createVirtualList } from './virtual.js';
import { el } from './dom.js';
import { linkButton } from './ui/link-button.js';

const ROW_HEIGHT = 28;
const CELL_WIDTH = 176;

const FORMULA = /^[=+@\t\r]/;
const NUMBER = /^-\d+(\.\d+)?$/;

/**
 * RFC 4180 text for the given rows; a null cell is an empty field. A cell a
 * spreadsheet would read as a formula (it starts with =, +, @, tab or CR, or
 * with - and is not a plain number) gets a leading apostrophe, so text from
 * the file cannot run in the reader's spreadsheet; negative amounts stay numeric.
 */
export function toCsv(names, rows) {
  const field = (cell) => {
    if (cell === null || cell === undefined) return '';
    const value = FORMULA.test(cell) || (cell.startsWith('-') && !NUMBER.test(cell)) ? `'${cell}` : cell;
    return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
  };
  const lines = [names.map(field).join(',')];
  for (const row of rows) lines.push(row.map(field).join(','));
  return `${lines.join('\r\n')}\r\n`;
}

/**
 * @param {HTMLElement} root
 * @param {{onJump: (segment: number) => void}} options
 */
export function createTables(root, { onJump }) {
  root.textContent = '';
  const select = el('select');
  select.id = 'table-select';
  const selectLabel = el('label', null, 'Table ');
  selectLabel.htmlFor = 'table-select';
  const filter = el('input');
  filter.type = 'search';
  filter.id = 'table-filter';
  filter.placeholder = 'Filter rows';
  filter.setAttribute('aria-label', 'Filter rows');
  const count = el('span', 'muted');
  count.setAttribute('role', 'status');
  const download = el('button', null, 'Download CSV');
  download.type = 'button';
  const bar = el('div', 'toolbar');
  const group = el('span', 'group');
  group.append(selectLabel, select);
  bar.append(group, filter, count, download);
  const scroller = el('div', 'grid');
  scroller.tabIndex = 0;
  scroller.setAttribute('role', 'table');
  scroller.setAttribute('aria-label', 'Table rows');
  root.append(bar, scroller);

  let view = null;
  let name = 'file';
  let table = null;
  let haystack = [];
  let shown = [];
  let list = null;
  let segmentColumn = -1;

  function cell(value, column, row) {
    const td = el('span', 'cell');
    td.setAttribute('role', 'cell');
    td.style.width = `${CELL_WIDTH}px`;
    if (value === null) {
      td.classList.add('null');
      td.textContent = '—';
      td.title = 'null';
    } else if (column === segmentColumn && value !== '') {
      const button = linkButton();
      button.textContent = value;
      button.dataset.segment = value;
      button.title = 'Show in Text';
      td.append(button);
    } else {
      td.textContent = value;
      td.title = value;
    }
    return td;
  }

  function renderRow(position) {
    const row = shown[position];
    const div = el('div', 'row');
    div.setAttribute('role', 'row');
    div.setAttribute('aria-rowindex', String(position + 2));
    if (position % 2) div.classList.add('odd');
    div.style.width = `${table.columns.length * CELL_WIDTH}px`;
    table.columns.forEach((column, c) => div.append(cell(column.values[row], c, row)));
    return div;
  }

  function applyFilter() {
    const needle = filter.value.trim().toLowerCase();
    shown = [];
    for (let row = 0; row < table.rows; row += 1) {
      if (!needle || haystack[row].includes(needle)) shown.push(row);
    }
    count.textContent = needle
      ? `${shown.length.toLocaleString()} of ${table.rows.toLocaleString()} rows`
      : `${table.rows.toLocaleString()} rows`;
    scroller.scrollTop = 0;
    list.setCount(shown.length);
    scroller.setAttribute('aria-rowcount', String(shown.length + 1));
  }

  function choose(tableName) {
    table = view.tables.find((t) => t.name === tableName);
    if (!table) return;
    segmentColumn = table.columns.findIndex((c) => c.name === 'segment');
    haystack = [];
    for (let row = 0; row < table.rows; row += 1) {
      haystack.push(table.columns.map((c) => c.values[row] ?? '').join('\u0001').toLowerCase());
    }
    const header = el('div', 'row head');
    header.setAttribute('role', 'row');
    header.setAttribute('aria-rowindex', '1');
    header.style.width = `${table.columns.length * CELL_WIDTH}px`;
    for (const column of table.columns) {
      const th = el('span', 'cell', column.name);
      th.setAttribute('role', 'columnheader');
      th.style.width = `${CELL_WIDTH}px`;
      th.title = column.name;
      header.append(th);
    }
    if (list) list.destroy();
    list = createVirtualList({ scroller, rowHeight: ROW_HEIGHT, render: renderRow, header });
    list.setWidth(`${table.columns.length * CELL_WIDTH}px`);
    scroller.setAttribute('aria-colcount', String(table.columns.length));
    applyFilter();
  }

  let timer = 0;
  filter.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => table && applyFilter(), 120);
  });
  select.addEventListener('change', () => choose(select.value));
  scroller.addEventListener('click', (event) => {
    const target = event.target instanceof Element ? event.target.closest('button[data-segment]') : null;
    if (target) onJump(Number(target.dataset.segment));
  });
  download.addEventListener('click', () => {
    if (!table) return;
    const rows = shown.map((row) => table.columns.map((c) => c.values[row]));
    const csv = toCsv(table.columns.map((c) => c.name), rows);
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
    const link = el('a');
    link.href = url;
    link.download = `${name.replace(/\.[^.]*$/, '')}-${table.name}.csv`;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });

  return {
    show(loaded) {
      view = loaded.view;
      name = loaded.name;
      select.textContent = '';
      for (const t of view.tables) {
        const option = el('option', null, `${t.name} (${t.rows.toLocaleString()})`);
        option.value = t.name;
        select.append(option);
      }
      filter.value = '';
      const first = view.tables.find((t) => t.name === 'claims') ?? view.tables[0];
      if (first) {
        select.value = first.name;
        choose(first.name);
      }
    },
    /** The selected table and the filter text, to put back after the same file is parsed again. */
    state() {
      return table ? { table: table.name, filter: filter.value } : null;
    },
    restore(saved) {
      if (!saved || !view || !view.tables.some((t) => t.name === saved.table)) return;
      select.value = saved.table;
      filter.value = saved.filter;
      choose(saved.table);
    },
    /** Called when the tab becomes visible: the grid can only measure itself then. */
    onShow() {
      if (list) list.refresh();
    },
    clear() {
      view = null;
      table = null;
      if (list) list.destroy();
      list = null;
      select.textContent = '';
      scroller.textContent = '';
      count.textContent = '';
    },
  };
}
