import * as http from 'node:http';
import {
  clearPendingInteraction,
  getPendingInteractions,
  getSessionHistory,
  runTask,
} from './runner.js';
import type { CompletionEvent, RunRequest } from './types.js';

const PORT = parseInt(process.env['HSM_CLAUDE_HARNESS_PORT'] ?? '3848', 10);
const HOST = process.env['HSM_CLAUDE_HARNESS_HOST'] ?? '127.0.0.1';

function writeNdjson(res: http.ServerResponse, event: CompletionEvent): void {
  res.write(JSON.stringify(event) + '\n');
}

function json(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
}

async function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')));
    req.on('error', reject);
  });
}

async function handleRun(
  req: http.IncomingMessage,
  res: http.ServerResponse,
): Promise<void> {
  let runReq: RunRequest;
  try {
    const body = await readBody(req);
    runReq = JSON.parse(body) as RunRequest;
  } catch (e) {
    res.writeHead(400, { 'Content-Type': 'text/plain' });
    res.end(`Invalid JSON: ${String(e)}`);
    return;
  }

  res.writeHead(200, {
    'Content-Type': 'application/x-ndjson',
    'Transfer-Encoding': 'chunked',
    'Cache-Control': 'no-cache',
    'X-Task-Id': runReq.task_id,
  });

  try {
    for await (const event of runTask(runReq)) {
      writeNdjson(res, event);
    }
  } catch (err) {
    const errEvent: CompletionEvent = {
      event_type: 'background_completion',
      success: false,
      message: String(err),
      ts_ms: Date.now(),
    };
    writeNdjson(res, errEvent);
  }

  res.end();
}

/**
 * Proxy an elicitation response through to the executor service.
 * Called by Rust when the operator submits an interaction response.
 */
async function handleElicitRespond(
  executionId: string,
  req: http.IncomingMessage,
  res: http.ServerResponse,
): Promise<void> {
  const executorUrl = process.env['HSM_EXECUTOR_URL']?.trim();
  if (!executorUrl) {
    json(res, 503, { error: 'HSM_EXECUTOR_URL not configured' });
    return;
  }

  const body = await readBody(req);
  try {
    const upstream = await fetch(
      `${executorUrl.replace(/\/$/, '')}/elicit/respond/${encodeURIComponent(executionId)}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
        signal: AbortSignal.timeout(10_000),
      },
    );
    const data = (await upstream.json()) as unknown;
    json(res, upstream.ok ? 200 : upstream.status, data);
  } catch (err) {
    json(res, 502, { error: String(err) });
  }
}

async function handleSessionHistory(taskId: string, res: http.ServerResponse): Promise<void> {
  const history = await getSessionHistory(taskId);
  json(res, 200, history);
}

async function handlePendingInteractions(taskId: string, res: http.ServerResponse): Promise<void> {
  const pending = await getPendingInteractions(taskId);
  json(res, 200, { task_id: taskId, pending });
}

async function handleResumeInteraction(
  taskId: string,
  req: http.IncomingMessage,
  res: http.ServerResponse,
): Promise<void> {
  const body = (await readBody(req).catch(() => '{}')).trim() || '{}';
  let parsed: { resume_token?: string; interaction_response?: unknown };
  try {
    parsed = JSON.parse(body) as { resume_token?: string; interaction_response?: unknown };
  } catch {
    json(res, 400, { error: 'invalid json body' });
    return;
  }
  const resumeToken = (parsed.resume_token ?? '').trim();
  if (!resumeToken) {
    json(res, 400, { error: 'resume_token required' });
    return;
  }
  const executorUrl = process.env['HSM_EXECUTOR_URL']?.trim();
  if (!executorUrl) {
    json(res, 503, { error: 'HSM_EXECUTOR_URL not configured' });
    return;
  }
  try {
    const upstream = await fetch(
      `${executorUrl.replace(/\/$/, '')}/elicit/respond/${encodeURIComponent(resumeToken)}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(parsed.interaction_response ?? {}),
        signal: AbortSignal.timeout(10_000),
      },
    );
    const data = (await upstream.json().catch(() => ({}))) as unknown;
    if (upstream.ok) {
      await clearPendingInteraction(taskId, resumeToken).catch(() => {});
    }
    json(res, upstream.ok ? 200 : upstream.status, data);
  } catch (err) {
    json(res, 502, { error: String(err) });
  }
}

export function startServer(): http.Server {
  const server = http.createServer(async (req, res) => {
    const url = req.url ?? '/';
    const method = req.method ?? 'GET';

    try {
      if (method === 'POST' && url === '/run') {
        await handleRun(req, res);

      // Elicitation response proxy: POST /elicit/respond/:executionId
      } else if (method === 'POST' && url.startsWith('/elicit/respond/')) {
        const executionId = decodeURIComponent(url.slice('/elicit/respond/'.length));
        await handleElicitRespond(executionId, req, res);

      } else if (method === 'GET' && url.startsWith('/sessions/') && url.endsWith('/history')) {
        const taskId = decodeURIComponent(
          url.slice('/sessions/'.length, url.length - '/history'.length),
        );
        await handleSessionHistory(taskId, res);

      } else if (method === 'GET' && url.startsWith('/sessions/') && url.endsWith('/pending')) {
        const taskId = decodeURIComponent(
          url.slice('/sessions/'.length, url.length - '/pending'.length),
        );
        await handlePendingInteractions(taskId, res);

      } else if (method === 'POST' && url.startsWith('/sessions/') && url.endsWith('/resume')) {
        const taskId = decodeURIComponent(
          url.slice('/sessions/'.length, url.length - '/resume'.length),
        );
        await handleResumeInteraction(taskId, req, res);

      } else if (method === 'GET' && url === '/health') {
        json(res, 200, {
          status: 'ok',
          service: 'claude-harness',
          port: PORT,
          executor: process.env['HSM_EXECUTOR_URL'] ?? null,
        });

      } else {
        res.writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('Not found');
      }
    } catch (err) {
      console.error('[claude-harness] unhandled error:', err);
      if (!res.headersSent) {
        res.writeHead(500);
      }
      res.end();
    }
  });

  server.listen(PORT, HOST, () => {
    console.log(
      `[claude-harness] listening on http://${HOST}:${PORT}  (pid ${process.pid})`,
    );
  });

  return server;
}
