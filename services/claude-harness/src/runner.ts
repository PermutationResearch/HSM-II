import { spawn } from 'node:child_process';
import * as readline from 'node:readline';
import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import type {
  AssistantMessage,
  ClaudeStreamLine,
  CompletionEvent,
  HarnessInteractionKind,
  HarnessState,
  RunRequest,
  UserMessage,
} from './types.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/** Absolute path to the executor MCP server script. */
const EXECUTOR_MCP_SCRIPT = resolve(
  __dirname,
  '../../executor/src/mcp-server.ts',
);

/** Absolute path to tsx in the executor's node_modules. */
const EXECUTOR_TSX_BIN = resolve(
  __dirname,
  '../../executor/node_modules/.bin/tsx',
);

/**
 * Write a temporary MCP config file so claude can discover the executor tools.
 * Returns the temp file path (caller must delete it after the run).
 */
async function writeMcpConfig(executorUrl: string, taskId: string): Promise<string> {
  const config = {
    mcpServers: {
      executor: {
        command: EXECUTOR_TSX_BIN,
        args: [EXECUTOR_MCP_SCRIPT],
        env: {
          HSM_EXECUTOR_URL: executorUrl,
        },
      },
    },
  };
  const path = join(tmpdir(), `hsm-mcp-${taskId}.json`);
  await writeFile(path, JSON.stringify(config), 'utf8');
  return path;
}

function now(): number {
  return Date.now();
}

function makeEvent(
  partial: Omit<CompletionEvent, 'success' | 'message' | 'ts_ms'> &
    Partial<Pick<CompletionEvent, 'success' | 'message' | 'ts_ms'>>,
): CompletionEvent {
  return {
    success: true,
    message: '',
    ts_ms: now(),
    ...partial,
  } as CompletionEvent;
}

type PendingInteraction = {
  kind: 'approval' | 'elicitation';
  resume_token: string;
  tool_name?: string;
  call_id?: string;
  message?: string;
  interaction?: Record<string, unknown>;
  ts_ms: number;
};

type SessionHistoryRecord = {
  task_id: string;
  session_id?: string;
  updated_at: string;
  last_state: HarnessState;
  last_resume_token?: string;
  pending: PendingInteraction[];
  raw_lines: string[];
  events: CompletionEvent[];
};

const SESSION_DIR = resolve(
  process.env['HSM_CLAUDE_HARNESS_SESSION_DIR']?.trim() || '.hsmii/claude-harness-sessions',
);

function sanitizeTaskKey(taskId: string): string {
  return taskId
    .replace(/[^a-zA-Z0-9._-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 160) || 'task';
}

function sessionPath(taskId: string): string {
  return join(SESSION_DIR, `${sanitizeTaskKey(taskId)}.json`);
}

async function loadSession(taskId: string): Promise<SessionHistoryRecord> {
  const path = sessionPath(taskId);
  try {
    const raw = await readFile(path, 'utf8');
    const parsed = JSON.parse(raw) as SessionHistoryRecord;
    if (parsed && parsed.task_id === taskId) {
      return parsed;
    }
  } catch {
    // Ignore and return new session skeleton.
  }
  return {
    task_id: taskId,
    updated_at: new Date().toISOString(),
    last_state: 'queued',
    pending: [],
    raw_lines: [],
    events: [],
  };
}

async function saveSession(session: SessionHistoryRecord): Promise<void> {
  await mkdir(SESSION_DIR, { recursive: true });
  session.updated_at = new Date().toISOString();
  const path = sessionPath(session.task_id);
  await writeFile(path, JSON.stringify(session, null, 2), 'utf8');
}

export async function getSessionHistory(taskId: string): Promise<SessionHistoryRecord> {
  return loadSession(taskId);
}

export async function getPendingInteractions(taskId: string): Promise<PendingInteraction[]> {
  const session = await loadSession(taskId);
  return session.pending;
}

export async function clearPendingInteraction(taskId: string, resumeToken: string): Promise<void> {
  const session = await loadSession(taskId);
  session.pending = session.pending.filter((p) => p.resume_token !== resumeToken);
  if (session.last_resume_token === resumeToken) {
    session.last_state = 'resuming';
  }
  await saveSession(session);
}

function stateEvent(
  event_type: string,
  harness_state: HarnessState,
  interaction_kind?: HarnessInteractionKind,
  resume_token?: string,
  interaction?: Record<string, unknown>,
  message = '',
): CompletionEvent {
  return makeEvent({
    event_type,
    success: event_type !== 'failed',
    message,
    harness_state,
    interaction_kind,
    resume_token,
    interaction,
    checkpoint_ref: resume_token ? `${event_type}:${resume_token}` : undefined,
  });
}

/**
 * Translate a single claude stream-json NDJSON line into zero or more
 * CompletionEvents using the same shape as Rust's runtime_control.rs.
 *
 * `toolNames` is a mutable map from tool_use_id → tool_name, built up as
 * `assistant` messages arrive so `tool_complete` events carry the name too.
 */
function translateLine(
  msg: ClaudeStreamLine,
  toolNames: Map<string, string>,
): CompletionEvent[] {
  switch (msg.type) {
    case 'system':
      // Emit an explicit state transition + synthetic tool_start for compatibility.
      return [
        stateEvent('state_transition', 'running', undefined, undefined, undefined, 'queued -> running'),
        makeEvent({
          event_type: 'tool_start',
          tool_name: 'claude_harness',
          call_id: msg.session_id,
          message: `Claude Code session ${msg.session_id} — model: ${msg.model}, tools: ${msg.tools.join(', ')}`,
        }),
      ];

    case 'assistant': {
      const am = msg as AssistantMessage;
      const events: CompletionEvent[] = [];
      for (const block of am.message.content) {
        if (block.type === 'tool_use') {
          // Track so tool_complete can carry the name
          toolNames.set(block.id, block.name);
          events.push(
            stateEvent(
              'waiting_tool',
              'waiting_tool',
              'tool_call',
              block.id,
              block.input,
              `${block.name} waiting_tool`,
            ),
          );
          if (block.name === 'resume') {
            events.push(
              stateEvent('resuming', 'resuming', 'operator_input', block.id, block.input, 'resume requested'),
            );
          }
          events.push(
            makeEvent({
              event_type: 'tool_start',
              tool_name: block.name,
              call_id: block.id,
              message: `${block.name} — starting`,
              input: block.input,
            }),
          );
        }
      }
      return events;
    }

    case 'user': {
      const um = msg as UserMessage;
      const events: CompletionEvent[] = [];
      if (Array.isArray(um.message.content)) {
        for (const block of um.message.content) {
          if (block.type !== 'tool_result') continue;
          const raw =
            typeof block.content === 'string'
              ? block.content
              : block.content.map((c) => c.text).join('');
          const isError = block.is_error === true;
          const resolvedName = toolNames.get(block.tool_use_id);
          events.push(
            makeEvent({
              event_type: isError ? 'tool_error' : 'tool_complete',
              tool_name: resolvedName,
              call_id: block.tool_use_id,
              success: !isError,
              message: raw.slice(0, 500),
              output_len: raw.length,
            }),
          );
          if (!isError) {
            const resumed = toolNames.get(block.tool_use_id) === 'resume';
            if (resumed) {
              events.push(stateEvent('state_transition', 'running', undefined, block.tool_use_id, undefined, 'resumed -> running'));
            }
          }
        }
      }
      return events;
    }

    case 'stream_event':
      return [
        makeEvent({
          event_type: 'stream_event',
          stream_event: msg.event,
        }),
      ];

    case 'result':
      if (msg.subtype === 'success') {
        return [
          stateEvent('state_transition', 'completed', undefined, undefined, undefined, 'running -> completed'),
          makeEvent({
            event_type: 'background_completion',
            success: true,
            message: msg.result ?? 'completed',
          }),
        ];
      } else {
        const errMsg =
          'errors' in msg && msg.errors.length > 0
            ? msg.errors.join('; ')
            : msg.subtype;
        return [
          stateEvent('failed', 'failed', undefined, undefined, undefined, errMsg),
          makeEvent({
            event_type: 'background_completion',
            success: false,
            message: errMsg,
          }),
        ];
      }

    default:
      return [];
  }
}

/**
 * Build the claude CLI args array from a RunRequest.
 * mcpConfigPath is optional — supplied when executor mode is active.
 */
function buildArgs(req: RunRequest, mcpConfigPath?: string): string[] {
  const args: string[] = [
    '-p',
    req.prompt,
    '--output-format',
    'stream-json',
    '--verbose',
  ];

  if (req.max_turns != null && req.max_turns > 0) {
    args.push('--max-turns', String(req.max_turns));
  }

  if (mcpConfigPath) {
    // Executor mode: inject the MCP config so claude discovers execute/resume
    args.push('--mcp-config', mcpConfigPath);

    // Only allow the executor tools plus safe built-ins.
    // If the caller already restricted tools, respect that; otherwise use the
    // executor default set.
    const toolList = req.allowed_tools?.length
      ? req.allowed_tools
      : ['execute', 'resume', 'Read', 'Bash', 'WebFetch'];
    args.push('--allowedTools', toolList.join(','));
  } else if (req.allowed_tools && req.allowed_tools.length > 0) {
    args.push('--allowedTools', req.allowed_tools.join(','));
  }

  return args;
}

function resolveClaudeCommand(): { cmd: string; argsPrefix: string[] } {
  const requireLocal = (process.env['HSM_CLAUDE_CLI_REQUIRE_LOCAL'] ?? '').trim().toLowerCase();
  const localRequired =
    requireLocal === '1' ||
    requireLocal === 'true' ||
    requireLocal === 'yes' ||
    requireLocal === 'on';
  const configured = process.env['HSM_CLAUDE_CLI_PATH']?.trim()
  if (configured && configured.length > 0) {
    if (!existsSync(configured)) {
      throw new Error(`HSM_CLAUDE_CLI_PATH not found: ${configured}`);
    }
    if (configured.endsWith('.js')) {
      return { cmd: 'node', argsPrefix: [configured] }
    }
    return { cmd: configured, argsPrefix: [] }
  }
  if (localRequired) {
    throw new Error(
      'HSM_CLAUDE_CLI_REQUIRE_LOCAL=1 but HSM_CLAUDE_CLI_PATH is not set. ' +
        'Point it to external/claude-code-from-npm/package/cli.js',
    );
  }
  return { cmd: 'claude', argsPrefix: [] }
}

/** Detect elicitation JSON in a tool result string and return it or null. */
function detectElicitation(raw: string): {
  resumeToken: string;
  interaction: Record<string, unknown>;
} | null {
  // Fast path — only parse if the string looks like a waiting_for_interaction payload
  if (!raw.includes('waiting_for_interaction')) return null;
  try {
    const parsed = JSON.parse(raw.trim()) as {
      status?: string;
      executionId?: string;
      interaction?: unknown;
    };
    if (parsed.status === 'waiting_for_interaction' && parsed.executionId) {
      return {
        resumeToken: parsed.executionId,
        interaction: (parsed.interaction as Record<string, unknown> | undefined) ?? {},
      };
    }
  } catch {
    // Not JSON or not the right shape
  }
  return null;
}

/** Detect approval-like blockage from a tool result and return token/message. */
function detectApproval(raw: string): { resumeToken: string; message: string } | null {
  const low = raw.toLowerCase();
  const looksBlocked =
    low.includes('approval required') ||
    low.includes('paused_approval') ||
    low.includes('denied by approval');
  if (!looksBlocked) return null;
  const tokenMatch =
    /execution[_\s-]?id[:=\s]+([0-9a-f-]{36})/i.exec(raw) ??
    /approval[_\s-]?key[:=\s]+([a-zA-Z0-9._:-]+)/i.exec(raw);
  const token = tokenMatch?.[1]?.trim();
  if (!token) return null;
  return { resumeToken: token, message: raw.slice(0, 500) };
}

/**
 * Run a task via `claude -p --output-format stream-json` and yield
 * CompletionEvents as they arrive.
 *
 * When HSM_EXECUTOR_URL is set the harness injects an MCP config that
 * exposes the executor's execute/resume tools to the Claude Code agent.
 */
export async function* runTask(req: RunRequest): AsyncGenerator<CompletionEvent> {
  const executorUrl = process.env['HSM_EXECUTOR_URL']?.trim() || null;
  const session = await loadSession(req.task_id);

  // Write MCP config for executor mode; undefined means native Claude Code tools
  let mcpConfigPath: string | undefined;
  if (executorUrl) {
    mcpConfigPath = await writeMcpConfig(executorUrl, req.task_id);
  }

  const args = buildArgs(req, mcpConfigPath);
  const claudeExec = resolveClaudeCommand();
  const spawnArgs = [...claudeExec.argsPrefix, ...args];
  const cwd = req.cwd ?? process.cwd();

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    ...(req.env ?? {}),
    ...(req.resume_token ? { HSM_RESUME_TOKEN: req.resume_token } : {}),
    ...(req.checkpoint_ref ? { HSM_CHECKPOINT_REF: req.checkpoint_ref } : {}),
    // Ensure claude doesn't prompt for auth interactively
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1',
  };

  const child = spawn(claudeExec.cmd, spawnArgs, {
    cwd,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  // Relay stderr to our own stderr for debugging
  child.stderr?.on('data', (chunk: Buffer) => {
    process.stderr.write(chunk);
  });

  // tool_use_id → tool_name — populated from `assistant` messages so that
  // `tool_complete` events carry the name the Rust evidence gates need.
  const toolNames = new Map<string, string>();

  const rl = readline.createInterface({
    input: child.stdout,
    crlfDelay: Infinity,
    terminal: false,
  });

  // We need to co-ordinate between readline events and the generator consumer.
  // We use a simple callback-driven queue with a promise gate.
  const eventQueue: CompletionEvent[] = [];
  let notifyConsumer: (() => void) | null = null;
  let procDone = false;
  let procError: Error | null = null;

  function enqueue(events: CompletionEvent[]): void {
    if (events.length === 0) return;
    eventQueue.push(...events);
    notifyConsumer?.();
    notifyConsumer = null;
  }

  if (req.resume_token) {
    const resumeEvents = [
      stateEvent(
        'resuming',
        'resuming',
        'operator_input',
        req.resume_token,
        req.checkpoint_ref ? { checkpoint_ref: req.checkpoint_ref } : undefined,
        'resume requested by caller',
      ),
    ];
    await recordEvents(resumeEvents);
    enqueue(resumeEvents);
  }

  async function recordRawLine(line: string): Promise<void> {
    // Keep most-recent rolling history (avoid unbounded growth).
    session.raw_lines.push(line);
    if (session.raw_lines.length > 3000) session.raw_lines.splice(0, session.raw_lines.length - 3000);
    await saveSession(session);
  }

  async function recordEvents(events: CompletionEvent[]): Promise<void> {
    if (events.length === 0) return;
    session.events.push(...events);
    if (session.events.length > 3000) session.events.splice(0, session.events.length - 3000);
    for (const ev of events) {
      if (ev.call_id && !session.session_id && ev.event_type === 'tool_start' && ev.tool_name === 'claude_harness') {
        session.session_id = ev.call_id;
      }
      if (ev.harness_state) session.last_state = ev.harness_state;
      if (ev.resume_token) session.last_resume_token = ev.resume_token;
    }
    await saveSession(session);
  }

  rl.on('line', async (line: string) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    await recordRawLine(trimmed);
    try {
      const msg = JSON.parse(trimmed) as ClaudeStreamLine;
      const events = translateLine(msg, toolNames);

      // Check every tool_complete event's message for executor elicitation payload
      const augmented: CompletionEvent[] = [];
      for (const ev of events) {
        augmented.push(ev);
        if (ev.event_type === 'tool_complete') {
          const elicit = detectElicitation(ev.message);
          if (elicit) {
            const waiting = stateEvent(
              'waiting_elicitation',
              'waiting_elicitation',
              'elicitation',
              elicit.resumeToken,
              elicit.interaction,
              'waiting_for_interaction',
            );
            const checkpoint = stateEvent(
              'checkpoint_write',
              'checkpointed',
              'elicitation',
              elicit.resumeToken,
              elicit.interaction,
              'elicitation checkpoint persisted',
            );
            session.pending.push({
              kind: 'elicitation',
              resume_token: elicit.resumeToken,
              tool_name: ev.tool_name ?? undefined,
              call_id: ev.call_id ?? undefined,
              message: waiting.message,
              interaction: elicit.interaction,
              ts_ms: Date.now(),
            });
            augmented.push(waiting, checkpoint);
          }
          const approval = detectApproval(ev.message);
          if (approval) {
            const waiting = stateEvent(
              'waiting_approval',
              'waiting_approval',
              'approval',
              approval.resumeToken,
              { message: approval.message, tool_name: ev.tool_name, call_id: ev.call_id },
              approval.message,
            );
            const checkpoint = stateEvent(
              'checkpoint_write',
              'checkpointed',
              'approval',
              approval.resumeToken,
              { message: approval.message, tool_name: ev.tool_name, call_id: ev.call_id },
              'approval checkpoint persisted',
            );
            session.pending.push({
              kind: 'approval',
              resume_token: approval.resumeToken,
              tool_name: ev.tool_name ?? undefined,
              call_id: ev.call_id ?? undefined,
              message: approval.message,
              interaction: { message: approval.message },
              ts_ms: Date.now(),
            });
            augmented.push(waiting, checkpoint);
          }
        }
      }
      await recordEvents(augmented);
      enqueue(augmented);
    } catch {
      // Non-JSON line from stdout guard divert or stray output — ignore.
    }
  });

  child.on('close', (code: number | null) => {
    procDone = true;
    if (code !== null && code !== 0) {
      procError = new Error(`claude exited with code ${code}`);
      void recordEvents([stateEvent('failed', 'failed', undefined, undefined, undefined, `exit code ${code}`)]);
    } else {
      void recordEvents([stateEvent('state_transition', 'completed', undefined, undefined, undefined, 'process closed')]);
    }
    notifyConsumer?.();
    notifyConsumer = null;
  });

  // Drain the queue, yielding events as they arrive.
  try {
    while (true) {
      while (eventQueue.length > 0) {
        yield eventQueue.shift()!;
      }
      if (procDone) {
        // Flush any last events that may have landed before the close callback fired.
        while (eventQueue.length > 0) {
          yield eventQueue.shift()!;
        }
        if (procError) throw procError;
        break;
      }
      // Park until enqueue() or close() fires notifyConsumer.
      await new Promise<void>((resolve) => {
        notifyConsumer = resolve;
      });
    }
  } finally {
    // Clean up temp MCP config file
    if (mcpConfigPath) {
      await unlink(mcpConfigPath).catch(() => {});
    }
  }
}
