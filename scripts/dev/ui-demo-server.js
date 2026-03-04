const http = require('http');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const repoRoot = path.resolve(__dirname, '..', '..');
const uiDir = path.join(repoRoot, 'control-plane-ui');

const OWNER = '0x0000000000000000000000000000000000000001';
const DEV_TOKEN = 'oclw_dev_owner_session';

const state = {
  templates: [
    { id: 'ops', name: 'Ops', mode: 'standard', description: 'Operational runtime setup' },
    { id: 'discord', name: 'Discord', mode: 'standard', description: 'Discord support flow' },
    { id: 'telegram', name: 'Telegram', mode: 'standard', description: 'Telegram support flow' },
  ],
  instances: [
    {
      id: 'inst-openclaw-1',
      name: 'openclaw-core',
      templatePackId: 'ops',
      clawVariant: 'openclaw',
      executionTarget: 'standard',
      status: 'running',
      owner: OWNER,
      createdAt: nowSec(),
      updatedAt: nowSec(),
      uiAccess: { publicUrl: 'https://openclaw.example.test', tunnelStatus: 'active', authMode: 'access_token', ownerOnly: true },
      runtime: {
        backend: 'docker', image: 'ghcr.io/openclaw/openclaw:latest', containerName: 'mock-openclaw', containerId: 'mock-openclaw-id',
        containerStatus: 'running', uiHostPort: 18789, uiLocalUrl: 'http://127.0.0.1:18789',
        uiAuthScheme: 'bearer', uiAuthEnvKey: 'SANDBOX_UI_BEARER_TOKEN', hasUiBearerToken: true,
        setupUrl: null, setupStatus: 'pending', setupCommand: 'openclaw onboard', setupInstructions: 'mock', lastError: null,
      },
    },
    {
      id: 'inst-nanoclaw-1',
      name: 'nanoclaw-terminal',
      templatePackId: 'ops',
      clawVariant: 'nanoclaw',
      executionTarget: 'standard',
      status: 'running',
      owner: OWNER,
      createdAt: nowSec(),
      updatedAt: nowSec(),
      uiAccess: { publicUrl: null, tunnelStatus: 'pending', authMode: 'access_token', ownerOnly: true },
      runtime: {
        backend: 'docker', image: 'nanoclaw-agent:latest', containerName: 'mock-nanoclaw', containerId: 'mock-nanoclaw-id',
        containerStatus: 'running', uiHostPort: null, uiLocalUrl: null,
        uiAuthScheme: 'bearer', uiAuthEnvKey: 'SANDBOX_UI_BEARER_TOKEN', hasUiBearerToken: true,
        setupUrl: null, setupStatus: 'pending', setupCommand: null, setupInstructions: 'run claude then /setup', lastError: null,
      },
    },
    {
      id: 'inst-ironclaw-tee-1',
      name: 'ironclaw-tee',
      templatePackId: 'ops',
      clawVariant: 'ironclaw',
      executionTarget: 'tee',
      status: 'running',
      owner: OWNER,
      createdAt: nowSec(),
      updatedAt: nowSec(),
      uiAccess: { publicUrl: 'https://ironclaw-tee.example.test', tunnelStatus: 'active', authMode: 'access_token', ownerOnly: true },
      runtime: {
        backend: 'docker', image: 'nearaidev/ironclaw-nearai-worker:latest', containerName: 'mock-ironclaw', containerId: 'mock-ironclaw-id',
        containerStatus: 'running', uiHostPort: 18791, uiLocalUrl: 'http://127.0.0.1:18791',
        uiAuthScheme: 'bearer', uiAuthEnvKey: 'SANDBOX_UI_BEARER_TOKEN', hasUiBearerToken: true,
        setupUrl: null, setupStatus: 'pending', setupCommand: 'ironclaw onboard', setupInstructions: 'mock', lastError: null,
      },
    },
  ],
  challenges: new Map(),
  chatByInstance: new Map(),
  sseBySession: new Map(),
  terminals: new Map(),
};

for (const inst of state.instances) {
  const sessionId = `chat-${inst.id}-1`;
  state.chatByInstance.set(inst.id, [{ id: sessionId, title: 'Primary Session', parentID: null, messages: [] }]);
}

function nowSec() {
  return Math.floor(Date.now() / 1000);
}

function json(res, status, body, headers = {}) {
  res.writeHead(status, {
    'cache-control': 'no-store',
    'content-type': 'application/json; charset=utf-8',
    ...headers,
  });
  res.end(JSON.stringify(body));
}

function text(res, status, body, contentType) {
  res.writeHead(status, {
    'cache-control': 'no-store',
    'content-type': contentType,
  });
  res.end(body);
}

function notFound(res) {
  json(res, 404, { error: { code: 'not_found', message: 'not found' }, requestId: 'dev' });
}

function parseBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf8');
      if (!raw) return resolve({});
      try { resolve(JSON.parse(raw)); } catch (err) { reject(err); }
    });
    req.on('error', reject);
  });
}

function bearerFromReq(req, urlObj) {
  const header = req.headers.authorization || '';
  const m = header.match(/^Bearer\s+(.+)$/i);
  if (m) return m[1].trim();
  const tokenQ = urlObj.searchParams.get('token');
  return tokenQ ? tokenQ.trim() : '';
}

function requireAuth(req, res, urlObj) {
  const token = bearerFromReq(req, urlObj);
  if (!token) {
    json(res, 401, { error: { code: 'unauthorized', message: 'missing Authorization bearer token' }, requestId: 'dev' });
    return false;
  }
  return true;
}

function uiContentType(filePath) {
  if (filePath.endsWith('.html')) return 'text/html; charset=utf-8';
  if (filePath.endsWith('.css')) return 'text/css; charset=utf-8';
  if (filePath.endsWith('.js') || filePath.endsWith('.mjs')) return 'application/javascript; charset=utf-8';
  if (filePath.endsWith('.json')) return 'application/json; charset=utf-8';
  if (filePath.endsWith('.svg')) return 'image/svg+xml';
  return 'application/octet-stream';
}

function serveUiFile(res, relPath) {
  const full = path.join(uiDir, relPath);
  if (!full.startsWith(uiDir)) return notFound(res);
  if (!fs.existsSync(full)) return notFound(res);
  const content = fs.readFileSync(full);
  text(res, 200, content, uiContentType(full));
}

function getInstance(id) {
  return state.instances.find((i) => i.id === id) || null;
}

function createMockInstance({ name, templatePackId, clawVariant, executionTarget }) {
  const id = `inst-${clawVariant}-${crypto.randomUUID().slice(0, 8)}`;
  const created = nowSec();
  const isTee = executionTarget === 'tee';
  const imageByVariant = {
    openclaw: 'ghcr.io/openclaw/openclaw:latest',
    nanoclaw: 'nanoclaw-agent:latest',
    ironclaw: 'nearaidev/ironclaw-nearai-worker:latest',
  };
  const hasUi = clawVariant !== 'nanoclaw';
  const uiPort = hasUi ? 18000 + Math.floor(Math.random() * 900) : null;

  return {
    id,
    name,
    templatePackId,
    clawVariant,
    executionTarget,
    status: 'running',
    owner: OWNER,
    createdAt: created,
    updatedAt: created,
    uiAccess: {
      publicUrl: hasUi ? `https://${name}.example.test` : null,
      tunnelStatus: hasUi ? 'active' : 'pending',
      authMode: 'access_token',
      ownerOnly: true,
    },
    runtime: {
      backend: 'docker',
      image: imageByVariant[clawVariant] || imageByVariant.openclaw,
      containerName: `mock-${id}`,
      containerId: `mock-${id}-cid`,
      containerStatus: 'running',
      uiHostPort: uiPort,
      uiLocalUrl: uiPort ? `http://127.0.0.1:${uiPort}` : null,
      uiAuthScheme: 'bearer',
      uiAuthEnvKey: 'SANDBOX_UI_BEARER_TOKEN',
      hasUiBearerToken: true,
      setupUrl: null,
      setupStatus: 'pending',
      setupCommand: clawVariant === 'ironclaw' ? 'ironclaw onboard' : clawVariant === 'openclaw' ? 'openclaw onboard' : null,
      setupInstructions: clawVariant === 'nanoclaw' ? 'run claude then /setup' : 'mock',
      lastError: null,
    },
  };
}

function getChats(instanceId) {
  if (!state.chatByInstance.has(instanceId)) {
    state.chatByInstance.set(instanceId, []);
  }
  return state.chatByInstance.get(instanceId);
}

function emitSessionEvent(sessionId, eventType, payload) {
  const subscribers = state.sseBySession.get(sessionId) || [];
  const frame = `event: ${eventType}\ndata: ${JSON.stringify(payload)}\n\n`;
  for (const res of subscribers) {
    res.write(frame);
  }
}

function addSseSubscriber(sessionId, res) {
  if (!state.sseBySession.has(sessionId)) {
    state.sseBySession.set(sessionId, []);
  }
  state.sseBySession.get(sessionId).push(res);
}

function removeSseSubscriber(sessionId, res) {
  const list = state.sseBySession.get(sessionId);
  if (!list) return;
  const next = list.filter((entry) => entry !== res);
  if (next.length === 0) state.sseBySession.delete(sessionId);
  else state.sseBySession.set(sessionId, next);
}

function writeSseHeaders(res) {
  res.writeHead(200, {
    'content-type': 'text/event-stream',
    'cache-control': 'no-store',
    connection: 'keep-alive',
  });
  res.write('retry: 1000\n\n');
}

const server = http.createServer(async (req, res) => {
  const urlObj = new URL(req.url, 'http://localhost');
  const p = urlObj.pathname;

  if (req.method === 'GET' && p === '/') return serveUiFile(res, 'index.html');
  if (req.method === 'GET' && p === '/app.js') return serveUiFile(res, 'app.js');
  if (req.method === 'GET' && p === '/styles.css') return serveUiFile(res, 'styles.css');
  if (req.method === 'GET' && p.startsWith('/assets/')) {
    const rel = p.slice(1);
    return serveUiFile(res, rel);
  }

  if (req.method === 'GET' && p === '/health') return json(res, 200, { status: 'ok' });
  if (req.method === 'GET' && p === '/favicon.ico') return text(res, 204, '', 'image/x-icon');

  if (req.method === 'POST' && p === '/auth/session/token') {
    return json(res, 200, {
      token: DEV_TOKEN,
      expiresAt: nowSec() + 3600,
      instanceId: 'inst-openclaw-1',
      owner: OWNER,
    });
  }

  if (req.method === 'POST' && p === '/auth/challenge') {
    const body = await parseBody(req).catch(() => ({}));
    const challengeId = `challenge-${crypto.randomUUID()}`;
    const msg = `OpenClaw owner auth\ninstance:${body.instanceId || ''}\nwallet:${body.walletAddress || ''}\nnonce:${challengeId}`;
    state.challenges.set(challengeId, { instanceId: body.instanceId || '', walletAddress: body.walletAddress || '' });
    return json(res, 200, { challengeId, message: msg, expiresAt: nowSec() + 300 });
  }

  if (req.method === 'POST' && p === '/auth/session/wallet') {
    const body = await parseBody(req).catch(() => ({}));
    const c = state.challenges.get(body.challengeId || '');
    if (!c) {
      return json(res, 400, { error: { code: 'bad_request', message: 'invalid challenge id' }, requestId: 'dev' });
    }
    return json(res, 200, {
      token: DEV_TOKEN,
      expiresAt: nowSec() + 3600,
      instanceId: c.instanceId || 'inst-openclaw-1',
      owner: c.walletAddress || OWNER,
    });
  }

  if (!requireAuth(req, res, urlObj)) return;

  if (req.method === 'GET' && p === '/templates') {
    return json(res, 200, { templatePacks: state.templates });
  }

  if (req.method === 'GET' && p === '/instances') {
    return json(res, 200, { instances: state.instances });
  }

  if (req.method === 'POST' && p === '/instances/dev/provision') {
    const body = await parseBody(req).catch(() => ({}));
    const name = String(body.name || '').trim();
    const templatePackId = String(body.templatePackId || '').trim();
    const clawVariant = String(body.clawVariant || '').trim();
    const executionTarget = String(body.executionTarget || '').trim();

    if (!name || !templatePackId || !['openclaw', 'nanoclaw', 'ironclaw'].includes(clawVariant)) {
      return json(res, 400, {
        error: {
          code: 'bad_request',
          message: 'name, templatePackId, and clawVariant(openclaw|nanoclaw|ironclaw) are required',
        },
        requestId: 'dev',
      });
    }

    const target = executionTarget === 'tee' ? 'tee' : 'standard';
    const created = createMockInstance({
      name,
      templatePackId,
      clawVariant,
      executionTarget: target,
    });
    state.instances.unshift(created);
    state.chatByInstance.set(created.id, [{ id: `chat-${created.id}-1`, title: 'Primary Session', parentID: null, messages: [] }]);
    return json(res, 200, created);
  }

  const byId = p.match(/^\/instances\/([^/]+)$/);
  if (req.method === 'GET' && byId) {
    const inst = getInstance(byId[1]);
    if (!inst) return notFound(res);
    return json(res, 200, inst);
  }

  const access = p.match(/^\/instances\/([^/]+)\/access$/);
  if (req.method === 'GET' && access) {
    const inst = getInstance(access[1]);
    if (!inst) return notFound(res);
    return json(res, 200, {
      instanceId: inst.id,
      authScheme: 'bearer',
      bearerToken: 'instance_ui_mock_token',
      uiLocalUrl: inst.runtime.uiLocalUrl,
      publicUrl: inst.uiAccess.publicUrl,
    });
  }

  const setup = p.match(/^\/instances\/([^/]+)\/setup\/start$/);
  if (req.method === 'POST' && setup) {
    const inst = getInstance(setup[1]);
    if (!inst) return notFound(res);
    inst.runtime.setupStatus = 'running';
    inst.updatedAt = nowSec();
    return json(res, 200, inst);
  }

  const teePk = p.match(/^\/instances\/([^/]+)\/tee\/public-key$/);
  if (req.method === 'GET' && teePk) {
    return json(res, 200, {
      instanceId: teePk[1],
      publicKey: {
        algorithm: 'x25519-hkdf-sha256',
        publicKeyBytes: [1, 2, 3, 4],
        attestation: { tee_type: 'Tdx', evidence: [10, 20], measurement: [30, 40], timestamp: nowSec() },
      },
    });
  }

  const teeAtt = p.match(/^\/instances\/([^/]+)\/tee\/attestation$/);
  if (req.method === 'GET' && teeAtt) {
    return json(res, 200, {
      instanceId: teeAtt[1],
      attestation: { tee_type: 'Tdx', evidence: [11, 22], measurement: [33, 44], timestamp: nowSec() },
    });
  }

  const teeSeal = p.match(/^\/instances\/([^/]+)\/tee\/sealed-secrets$/);
  if (req.method === 'POST' && teeSeal) {
    const body = await parseBody(req).catch(() => ({}));
    const secrets = body?.sealedSecret?.ciphertext;
    return json(res, 200, {
      instanceId: teeSeal[1],
      success: true,
      secretsCount: Array.isArray(secrets) && secrets.length > 0 ? 1 : 0,
    });
  }

  const ssh = p.match(/^\/instances\/([^/]+)\/ssh$/);
  if ((req.method === 'POST' || req.method === 'DELETE') && ssh) {
    return json(res, 200, { ok: true });
  }

  const termCreate = p.match(/^\/instances\/([^/]+)\/terminals$/);
  if (req.method === 'POST' && termCreate) {
    const sessionId = `term-${crypto.randomUUID()}`;
    state.terminals.set(sessionId, { instanceId: termCreate[1], owner: OWNER, createdAt: Date.now() });
    return json(res, 200, { data: { sessionId } });
  }

  const termStream = p.match(/^\/instances\/([^/]+)\/terminals\/([^/]+)\/stream$/);
  if (req.method === 'GET' && termStream) {
    writeSseHeaders(res);
    const sessionId = termStream[2];
    const key = `terminal:${sessionId}`;
    addSseSubscriber(key, res);
    res.write(`data: Connected to instance terminal.\\n\\n\n\n`);
    req.on('close', () => removeSseSubscriber(key, res));
    return;
  }

  const termExec = p.match(/^\/instances\/([^/]+)\/terminals\/([^/]+)\/execute$/);
  if (req.method === 'POST' && termExec) {
    const body = await parseBody(req).catch(() => ({}));
    const command = String(body.command || '').trim();
    const stdout = `mock exec> ${command}\n`;
    const key = `terminal:${termExec[2]}`;
    emitSessionEvent(key, 'message', stdout);
    return json(res, 200, { exitCode: 0, stdout, stderr: '' });
  }

  const termDelete = p.match(/^\/instances\/([^/]+)\/terminals\/([^/]+)$/);
  if (req.method === 'DELETE' && termDelete) {
    state.terminals.delete(termDelete[2]);
    return json(res, 200, { ok: true });
  }

  const chatSessions = p.match(/^\/instances\/([^/]+)\/session\/sessions$/);
  if (req.method === 'GET' && chatSessions) {
    const sessions = getChats(chatSessions[1]).map((s) => ({ id: s.id, title: s.title, parentID: s.parentID }));
    return json(res, 200, sessions);
  }
  if (req.method === 'POST' && chatSessions) {
    const body = await parseBody(req).catch(() => ({}));
    const title = String(body.title || 'Session').trim() || 'Session';
    const session = { id: `chat-${crypto.randomUUID()}`, title, parentID: null, messages: [] };
    getChats(chatSessions[1]).push(session);
    return json(res, 200, { id: session.id, title: session.title, parentID: session.parentID });
  }

  const chatSessionById = p.match(/^\/instances\/([^/]+)\/session\/sessions\/([^/]+)$/);
  if (chatSessionById && req.method === 'PATCH') {
    const body = await parseBody(req).catch(() => ({}));
    const sessions = getChats(chatSessionById[1]);
    const session = sessions.find((s) => s.id === chatSessionById[2]);
    if (!session) return notFound(res);
    session.title = String(body.title || session.title).trim() || session.title;
    return json(res, 200, { id: session.id, title: session.title, parentID: session.parentID });
  }
  if (chatSessionById && req.method === 'DELETE') {
    const sessions = getChats(chatSessionById[1]);
    const idx = sessions.findIndex((s) => s.id === chatSessionById[2]);
    if (idx === -1) return notFound(res);
    sessions.splice(idx, 1);
    return json(res, 200, { ok: true });
  }

  const chatMessages = p.match(/^\/instances\/([^/]+)\/session\/sessions\/([^/]+)\/messages$/);
  if (chatMessages && req.method === 'GET') {
    const sessions = getChats(chatMessages[1]);
    const session = sessions.find((s) => s.id === chatMessages[2]);
    if (!session) return notFound(res);
    return json(res, 200, session.messages);
  }
  if (chatMessages && req.method === 'POST') {
    const body = await parseBody(req).catch(() => ({}));
    const sessions = getChats(chatMessages[1]);
    const session = sessions.find((s) => s.id === chatMessages[2]);
    if (!session) return notFound(res);

    const userText = body?.parts?.[0]?.text || 'hello';
    const userMsg = {
      info: { id: `msg-u-${crypto.randomUUID()}`, role: 'user', timestamp: new Date().toISOString() },
      parts: [{ type: 'text', text: String(userText) }],
    };
    session.messages.push(userMsg);

    const assistantId = `msg-a-${crypto.randomUUID()}`;
    const assistantText = `Mock assistant processed: ${String(userText)}`;
    const assistantMsg = {
      info: { id: assistantId, role: 'assistant', timestamp: new Date().toISOString() },
      parts: [{ type: 'text', text: assistantText }],
    };
    session.messages.push(assistantMsg);

    emitSessionEvent(session.id, 'message.updated', { id: assistantId, role: 'assistant' });
    emitSessionEvent(session.id, 'message.part.updated', { type: 'text', text: assistantText });
    emitSessionEvent(session.id, 'session.idle', { timestamp: Date.now() });

    return json(res, 200, { ok: true });
  }

  const chatAbort = p.match(/^\/instances\/([^/]+)\/session\/sessions\/([^/]+)\/abort$/);
  if (chatAbort && req.method === 'POST') {
    emitSessionEvent(chatAbort[2], 'session.idle', { timestamp: Date.now() });
    return json(res, 200, { ok: true });
  }

  const chatEvents = p.match(/^\/instances\/([^/]+)\/session\/events$/);
  if (chatEvents && req.method === 'GET') {
    const sessionId = urlObj.searchParams.get('sessionId');
    if (!sessionId) {
      return json(res, 400, { error: { code: 'bad_request', message: 'missing sessionId' }, requestId: 'dev' });
    }
    writeSseHeaders(res);
    addSseSubscriber(sessionId, res);
    req.on('close', () => removeSseSubscriber(sessionId, res));
    return;
  }

  return notFound(res);
});

const DEMO_PORT = Number(process.env.OPENCLAW_UI_DEMO_PORT || '8787');
const DEMO_HOST = process.env.OPENCLAW_UI_DEMO_HOST || '0.0.0.0';

server.listen(DEMO_PORT, DEMO_HOST, () => {
  console.log(`OpenClaw UI demo listening on http://${DEMO_HOST}:${DEMO_PORT}`);
  console.log(`Bearer token: ${DEV_TOKEN}`);
});
