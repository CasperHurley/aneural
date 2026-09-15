const tty = process.stdout.isTTY === true && !process.env.NO_COLOR;
const ESC = String.fromCharCode(27);

function paint(code: string, s: string): string {
  return tty ? `${ESC}[${code}m${s}${ESC}[0m` : s;
}

export const c = {
  dim: (s: string): string => paint('2', s),
  bold: (s: string): string => paint('1', s),
  green: (s: string): string => paint('32', s),
  yellow: (s: string): string => paint('33', s),
  red: (s: string): string => paint('31', s),
  accent: (s: string): string => paint('38;2;159;209;143', s),
};

export function table(rows: string[][], header?: string[]): string {
  const all = header ? [header, ...rows] : rows;
  const widths: number[] = [];
  for (const row of all) {
    row.forEach((cell, i) => {
      widths[i] = Math.max(widths[i] ?? 0, cell.length);
    });
  }
  const line = (row: string[]): string =>
    row
      .map((cell, i) => cell.padEnd(widths[i] ?? 0))
      .join('  ')
      .trimEnd();
  const out = all.map(line);
  if (header) out.splice(1, 0, widths.map((w) => '-'.repeat(w)).join('  '));
  return out.join('\n');
}

export function json(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

export function fail(message: string, code = 1): never {
  process.stderr.write(`${c.red('error')} ${message}\n`);
  process.exit(code);
}
