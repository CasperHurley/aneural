#!/usr/bin/env node
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import * as core from '@aneural/core';
import { Command } from 'commander';
import { c, fail, json, table } from './output.js';
import { fetchRegistry, fetchSpore } from './registry.js';

const VERSION = '0.1.0';

const program = new Command()
  .name('aneural')
  .description('Aneural: a living map of everything you are building.')
  .version(VERSION)
  .option('--root <dir>', 'workspace root (default: nearest .aneural above the cwd)');

interface Globals {
  root?: string;
}

function resolveRoot(cmd: Command, required = true): string {
  const g = cmd.optsWithGlobals<Globals>();
  if (g.root) {
    const found = core.findWorkspace(path.resolve(g.root));
    if (found) return found;
    if (required) fail(`no .aneural workspace at or above ${g.root} (run \`aneural init\`)`);
    return path.resolve(g.root);
  }
  const found = core.findWorkspace(process.cwd());
  if (found) return found;
  if (required)
    fail('no .aneural workspace found above the current directory (run `aneural init`)');
  return process.cwd();
}

function configPath(root: string): string {
  return path.join(root, '.aneural', 'config.json');
}

function readConfig(root: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(configPath(root), 'utf8')) as Record<string, unknown>;
}

function writeConfig(root: string, config: Record<string, unknown>): void {
  fs.writeFileSync(configPath(root), `${JSON.stringify(config, null, 2)}\n`);
}

function enabledSpores(config: Record<string, unknown>): string[] {
  const spores = (config.spores ?? {}) as { enabled?: string[] };
  return spores.enabled ?? [];
}

function setEnabledSpores(config: Record<string, unknown>, enabled: string[]): void {
  const spores = (config.spores ?? {}) as Record<string, unknown>;
  spores.enabled = enabled;
  config.spores = spores;
}

const MCP_SERVER_ENTRY = { command: 'npx', args: ['-y', 'aneural', 'mcp'] };

function writeMcpJson(dir: string): string {
  const file = path.join(dir, '.mcp.json');
  let existing: Record<string, unknown> = {};
  if (fs.existsSync(file)) {
    try {
      existing = JSON.parse(fs.readFileSync(file, 'utf8')) as Record<string, unknown>;
    } catch {
      fail(`${file} exists but is not valid JSON; fix or remove it first`);
    }
  }
  const servers = (existing.mcpServers ?? {}) as Record<string, unknown>;
  servers.aneural = MCP_SERVER_ENTRY;
  existing.mcpServers = servers;
  fs.writeFileSync(file, `${JSON.stringify(existing, null, 2)}\n`);
  return file;
}

// ---- init ------------------------------------------------------------------

program
  .command('init [dir]')
  .description('create a .aneural/ workspace directory')
  .option('--name <name>', 'workspace display name')
  .option('--force', 'overwrite an existing config.json')
  .option('--claude', 'also register the MCP server in <dir>/.mcp.json for Claude Code')
  .option('--codex', 'print the Codex config.toml snippet for the MCP server')
  .action(
    (
      dir: string | undefined,
      opts: { name?: string; force?: boolean; claude?: boolean; codex?: boolean },
    ) => {
      const target = path.resolve(dir ?? program.opts<Globals>().root ?? process.cwd());
      try {
        const info = core.initWorkspace(target, { name: opts.name, force: opts.force ?? false });
        process.stdout.write(
          `${c.green('created')} ${info.aneuralDir}  ${c.dim(`(workspace "${info.name}")`)}\n`,
        );
      } catch (e) {
        fail((e as Error).message);
      }
      if (opts.claude) {
        const file = writeMcpJson(target);
        process.stdout.write(`${c.green('wrote')} ${file}  ${c.dim('→ mcpServers.aneural')}\n`);
      }
      if (opts.codex) {
        process.stdout.write(
          `\nAdd to ~/.codex/config.toml:\n\n[mcp_servers.aneural]\ncommand = "npx"\nargs = ["-y", "aneural", "mcp"]\n\n`,
        );
      }
      process.stdout.write(`${c.dim('next:')} aneural index\n`);
    },
  );

// ---- index -----------------------------------------------------------------

program
  .command('index')
  .description('index the workspace (incremental)')
  .option('--full', 're-analyse every file')
  .option('--watch', 'keep running and apply changes live')
  .option('--json', 'machine-readable output')
  .action(async (opts: { full?: boolean; watch?: boolean; json?: boolean }, cmd: Command) => {
    const root = resolveRoot(cmd);
    if (opts.watch) {
      let printedStats = false;
      const handle = core.watch(root, (ev) => {
        if (ev.type === 'delta' && printedStats) {
          const d = ev.delta as {
            nodes: unknown[];
            edges: unknown[];
            removedNodeIds: unknown[];
            removedEdges: unknown[];
          };
          const removed = d.removedNodeIds.length + d.removedEdges.length;
          if (d.nodes.length + d.edges.length + removed > 0) {
            process.stdout.write(
              `${c.dim(new Date().toISOString())} ${c.green(`+${d.nodes.length} nodes`)} ${c.accent(`+${d.edges.length} edges`)}${removed ? ` ${c.yellow(`-${removed} removed`)}` : ''}\n`,
            );
          }
        } else if (ev.type === 'indexComplete') {
          printedStats = true;
          process.stdout.write(
            opts.json
              ? `${JSON.stringify(ev.stats)}\n`
              : `${formatStats(ev.stats as core.IndexStats)}\n`,
          );
        } else if (ev.type === 'watching') {
          process.stdout.write(`${c.accent('watching')} ${root} ${c.dim('(ctrl-c to stop)')}\n`);
        } else if (ev.type === 'error') {
          process.stderr.write(`${c.red('engine')} ${ev.message}\n`);
        }
      });
      const stop = (): void => {
        handle.stop();
        process.exit(0);
      };
      process.on('SIGINT', stop);
      process.on('SIGTERM', stop);
      await new Promise(() => {});
      return;
    }
    const stats = await core.indexWorkspace(root, { full: opts.full ?? false });
    process.stdout.write(opts.json ? `${json(stats)}\n` : `${formatStats(stats)}\n`);
  });

function formatStats(s: core.IndexStats): string {
  return `${c.green('indexed')} ${s.filesIndexed} files ${c.dim(`(${s.filesSkipped} unchanged)`)} → ${c.bold(String(s.nodes))} nodes, ${c.bold(String(s.edges))} edges${s.unresolved ? c.yellow(`, ${s.unresolved} unresolved imports`) : ''} ${c.dim(`in ${s.durationMs} ms`)}`;
}

// ---- query -----------------------------------------------------------------

program
  .command('query <text>')
  .description('search nodes by label, path or id')
  .option('-k, --kind <kinds...>', 'restrict to node kinds')
  .option('-n, --limit <n>', 'max results', '50')
  .option('--json', 'machine-readable output')
  .action(
    (text: string, opts: { kind?: string[]; limit: string; json?: boolean }, cmd: Command) => {
      const root = resolveRoot(cmd);
      const nodes = core.queryNodes(root, { text, kinds: opts.kind, limit: Number(opts.limit) });
      if (opts.json) {
        process.stdout.write(`${json(nodes)}\n`);
        return;
      }
      if (nodes.length === 0) {
        process.stdout.write(`${c.dim('no nodes match')} ${text}\n`);
        return;
      }
      process.stdout.write(
        `${table(
          nodes.map((n) => [n.kind, n.id, n.label]),
          ['kind', 'id', 'label'],
        )}\n`,
      );
    },
  );

// ---- neighbors -------------------------------------------------------------

program
  .command('neighbors <id>')
  .description("show a node's neighbourhood")
  .option('-d, --depth <n>', 'hops', '1')
  .option('--direction <dir>', 'in | out | both', 'both')
  .option('--json', 'machine-readable output')
  .action(
    (id: string, opts: { depth: string; direction: string; json?: boolean }, cmd: Command) => {
      const root = resolveRoot(cmd);
      const g = core.neighborhood(root, [id], {
        depth: Number(opts.depth),
        direction: opts.direction,
        limit: 500,
      });
      if (opts.json) {
        process.stdout.write(`${json(g)}\n`);
        return;
      }
      const labels = new Map(g.nodes.map((n) => [n.id, n.label]));
      process.stdout.write(`${c.bold(id)}\n`);
      for (const e of g.edges) {
        const other = e.src === id ? e.dst : e.src;
        const arrow = e.src === id ? '→' : '←';
        process.stdout.write(
          `  ${arrow} ${c.accent(e.kind.padEnd(11))} ${other} ${c.dim(labels.get(other) ?? '')}\n`,
        );
      }
      if (g.truncated) process.stdout.write(`${c.dim('(truncated)')}\n`);
    },
  );

// ---- focus -----------------------------------------------------------------

const focus = program
  .command('focus')
  .description('inspect or set the focus handed to MCP clients');

focus
  .command('show', { isDefault: true })
  .option('--json', 'machine-readable output')
  .action((opts: { json?: boolean }, cmd: Command) => {
    const root = resolveRoot(cmd);
    const f = core.readFocus(root);
    if (!f) {
      process.stdout.write(
        `${c.dim('no focus yet')} — select a node in the GUI or run \`aneural focus set <id>\`\n`,
      );
      return;
    }
    if (opts.json) {
      process.stdout.write(`${json(f)}\n`);
      return;
    }
    const sel = f.selection as { primary?: string; pinned?: string[] };
    const ids = [sel.primary, ...(sel.pinned ?? [])].filter((x): x is string => Boolean(x));
    const nodes = core.getNodes(root, ids);
    process.stdout.write(`${c.dim('updated')} ${f.updatedAt}\n`);
    process.stdout.write(`${c.bold('primary')} ${sel.primary ?? c.dim('none')}\n`);
    process.stdout.write(
      `${c.bold('pinned')}  ${(sel.pinned ?? []).join(', ') || c.dim('none')}\n`,
    );
    if (nodes.length) process.stdout.write(`${table(nodes.map((n) => [n.kind, n.id, n.label]))}\n`);
    process.stdout.write(
      `${c.bold('visible')} ${f.visibleNodeIds.length} nodes${f.notes ? `\n${c.bold('notes')}   ${f.notes}` : ''}\n`,
    );
  });

focus
  .command('set <ids...>')
  .description('set the primary (first id) and pinned (rest) selection')
  .action((ids: string[], _opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    const [primary, ...pinned] = ids;
    const missing = ids.filter((id) => !core.getNode(root, id));
    if (missing.length) fail(`unknown node id(s): ${missing.join(', ')}`);
    const previous = core.readFocus(root);
    core.writeFocus(root, {
      version: 1,
      updatedAt: new Date().toISOString(),
      workspace: root,
      filters: previous?.filters ?? {},
      selection: { primary, pinned },
      neighborhood: previous?.neighborhood ?? { depth: 1, direction: 'both' },
      visibleNodeIds: ids,
      notes: previous?.notes ?? '',
    });
    process.stdout.write(
      `${c.green('focus')} ${primary}${pinned.length ? c.dim(` +${pinned.length} pinned`) : ''}\n`,
    );
  });

focus.command('clear').action((_opts: unknown, cmd: Command) => {
  const root = resolveRoot(cmd);
  const file = path.join(root, '.aneural', 'state', 'focus.json');
  fs.rmSync(file, { force: true });
  process.stdout.write(`${c.green('cleared')} ${file}\n`);
});

// ---- spores ----------------------------------------------------------------

const spores = program.command('spores').description('list, install and validate spores');

spores
  .command('list', { isDefault: true })
  .option('--json', 'machine-readable output')
  .action((opts: { json?: boolean }, cmd: Command) => {
    const root = resolveRoot(cmd);
    const list = core.listSpores(root);
    if (opts.json) {
      process.stdout.write(`${json(list)}\n`);
      return;
    }
    process.stdout.write(
      `${table(
        list.map((s) => [
          s.enabled ? c.green('on') : c.dim('off'),
          s.name,
          s.version,
          s.location,
          s.nodeKinds.join(','),
          s.description,
        ]),
        ['', 'name', 'version', 'from', 'kinds', 'description'],
      )}\n`,
    );
  });

spores
  .command('validate <path>')
  .description('check a spore.json manifest')
  .action((file: string) => {
    const problems = core.validateSpore(path.resolve(file));
    if (problems.length === 0) {
      process.stdout.write(`${c.green('valid')} ${file}\n`);
      return;
    }
    for (const p of problems) process.stdout.write(`${c.red('✗')} ${p}\n`);
    process.exit(1);
  });

async function installSpore(root: string, name: string, registryUrl: string): Promise<void> {
  const reg = await fetchRegistry(registryUrl);
  const entry = reg.spores.find((s) => s.name === name);
  if (!entry) fail(`spore "${name}" is not in the registry (${registryUrl})`);
  const fetched = await fetchSpore(entry);
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'aneural-spore-'));
  const tmpFile = path.join(tmp, 'spore.json');
  fs.writeFileSync(tmpFile, fetched.text);
  const problems = core.validateSpore(tmpFile);
  fs.rmSync(tmp, { recursive: true, force: true });
  if (problems.length) fail(`spore "${name}" is invalid:\n  ${problems.join('\n  ')}`);
  const dir = path.join(root, '.aneural', 'spores', name);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'spore.json'), fetched.text);
  const config = readConfig(root);
  const enabled = enabledSpores(config);
  if (!enabled.includes(name)) setEnabledSpores(config, [...enabled, name]);
  writeConfig(root, config);
  process.stdout.write(`${c.green('installed')} ${name}@${fetched.manifest.version} → ${dir}\n`);
}

spores
  .command('add <name>')
  .description('install a spore from the marketplace registry')
  .action(async (name: string, _opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    const config = readConfig(root);
    const registry = ((config.spores ?? {}) as { registry?: string }).registry;
    if (!registry) fail('config.spores.registry is not set');
    try {
      await installSpore(root, name, registry);
    } catch (e) {
      fail((e as Error).message);
    }
  });

spores
  .command('remove <name>')
  .description('uninstall a workspace spore (or disable a builtin)')
  .action((name: string, _opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    fs.rmSync(path.join(root, '.aneural', 'spores', name), { recursive: true, force: true });
    const config = readConfig(root);
    setEnabledSpores(
      config,
      enabledSpores(config).filter((s) => s !== name),
    );
    writeConfig(root, config);
    process.stdout.write(`${c.green('removed')} ${name}\n`);
  });

spores
  .command('update')
  .description('re-fetch installed workspace spores')
  .action(async (_opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    const config = readConfig(root);
    const registry = ((config.spores ?? {}) as { registry?: string }).registry;
    if (!registry) fail('config.spores.registry is not set');
    const installed = core.listSpores(root).filter((s) => s.location === 'workspace');
    if (installed.length === 0) {
      process.stdout.write(`${c.dim('no workspace spores installed')}\n`);
      return;
    }
    for (const s of installed) {
      try {
        await installSpore(root, s.name, registry);
      } catch (e) {
        process.stderr.write(`${c.red('failed')} ${s.name}: ${(e as Error).message}\n`);
      }
    }
  });

// ---- mcp -------------------------------------------------------------------

program
  .command('mcp')
  .description('serve the focused graph over MCP (stdio)')
  .action(async (_opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    const { serveAneuralStdio } = await import('@aneural/mcp');
    const handle = await serveAneuralStdio({ root });
    const shutdown = (): void => {
      handle.close().finally(() => process.exit(0));
    };
    process.on('SIGINT', shutdown);
    process.on('SIGTERM', shutdown);
  });

// ---- doctor ----------------------------------------------------------------

program
  .command('doctor')
  .description('check the workspace, spores, icons and unresolved imports')
  .option('--json', 'machine-readable output')
  .action((opts: { json?: boolean }, cmd: Command) => {
    const root = resolveRoot(cmd);
    const diags = core.doctor(root);
    if (opts.json) {
      process.stdout.write(`${json(diags)}\n`);
    } else if (diags.length === 0) {
      process.stdout.write(`${c.green('healthy')} nothing to report\n`);
    } else {
      const paint: Record<string, (s: string) => string> = {
        error: c.red,
        warning: c.yellow,
        info: c.dim,
      };
      for (const level of ['error', 'warning', 'info']) {
        const rows = diags.filter((d) => d.level === level);
        if (rows.length === 0) continue;
        process.stdout.write(`${(paint[level] ?? c.dim)(`${level} (${rows.length})`)}\n`);
        for (const d of rows)
          process.stdout.write(
            `  ${c.dim(d.category.padEnd(10))} ${d.message}${d.path ? c.dim(`  ${d.path}`) : ''}\n`,
          );
      }
    }
    if (diags.some((d) => d.level === 'error')) process.exit(1);
  });

// ---- open ------------------------------------------------------------------

program
  .command('open')
  .description('open the workspace in the Aneural GUI')
  .action((_opts: unknown, cmd: Command) => {
    const root = resolveRoot(cmd);
    const child = spawn('aneural-gui', [root], { stdio: 'ignore', detached: true });
    child.on('error', () => {
      process.stdout.write(
        `${c.yellow('aneural-gui not found on PATH')}\nrun it from the repo: cargo run -p aneural-gui -- ${root}\n`,
      );
    });
    child.on('spawn', () => {
      child.unref();
      process.stdout.write(`${c.green('opened')} ${root}\n`);
    });
  });

program.parseAsync(process.argv).catch((e: Error) => fail(e.message));
