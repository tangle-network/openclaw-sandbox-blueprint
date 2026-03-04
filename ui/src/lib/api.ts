export const TOKEN_STORAGE_KEY = 'openclawBearerToken';

export interface TemplatePack {
  id: string;
  name: string;
  mode: string;
  description: string;
}

export interface InstanceView {
  id: string;
  name: string;
  templatePackId: string;
  clawVariant: 'openclaw' | 'nanoclaw' | 'ironclaw' | string;
  executionTarget: 'standard' | 'tee' | string;
  status: string;
  owner: string;
  createdAt: number;
  updatedAt: number;
  uiAccess: {
    publicUrl: string | null;
    tunnelStatus: string;
    authMode: string;
    ownerOnly: boolean;
  };
  runtime: {
    backend: string;
    image: string | null;
    containerName: string | null;
    containerId: string | null;
    containerStatus: string | null;
    uiHostPort: number | null;
    uiLocalUrl: string | null;
    uiAuthScheme: string | null;
    uiAuthEnvKey: string | null;
    hasUiBearerToken: boolean;
    setupUrl: string | null;
    setupStatus: string | null;
    setupCommand: string | null;
    setupInstructions: string | null;
    lastError: string | null;
  };
}

export interface ApiSessionToken {
  token: string;
  expiresAt: number;
  instanceId: string;
  owner: string;
}

export interface InstanceAccess {
  instanceId: string;
  authScheme: string;
  bearerToken: string;
  uiLocalUrl: string | null;
  publicUrl: string | null;
}

export interface TeePublicKeyResponse {
  instanceId: string;
  publicKey: unknown;
}

export interface TeeAttestationResponse {
  instanceId: string;
  attestation: unknown;
}

export interface TeeSealedSecretsResponse {
  instanceId: string;
  success: boolean;
  secretsCount: number;
  error?: string;
}

export interface ExecuteTerminalResponse {
  exitCode: number;
  stdout: string;
  stderr: string;
}

export interface SessionSummary {
  id: string;
  title: string;
  parentID?: string;
}

interface ApiErrorBody {
  error?: {
    code?: string;
    message?: string;
  };
  requestId?: string;
}

export function loadSavedToken(): string {
  return localStorage.getItem(TOKEN_STORAGE_KEY) ?? '';
}

export function saveToken(token: string): void {
  const normalized = token.trim();
  if (normalized.length === 0) {
    localStorage.removeItem(TOKEN_STORAGE_KEY);
    return;
  }
  localStorage.setItem(TOKEN_STORAGE_KEY, normalized);
}

export function parseByteSequence(raw: string): number[] {
  const trimmed = `${raw ?? ''}`.trim();
  if (!trimmed) return [];

  if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
    const parsed = JSON.parse(trimmed);
    if (!Array.isArray(parsed)) {
      throw new Error('Expected JSON array for byte sequence.');
    }
    return parsed.map((value) => {
      const num = Number(value);
      if (!Number.isInteger(num) || num < 0 || num > 255) {
        throw new Error(`Invalid byte value: ${value}`);
      }
      return num;
    });
  }

  const noPrefix = trimmed.startsWith('0x') ? trimmed.slice(2) : trimmed;
  if (!/^[0-9a-fA-F]*$/.test(noPrefix) || noPrefix.length % 2 !== 0) {
    throw new Error('Byte sequence must be JSON array or even-length hex string.');
  }
  const out: number[] = [];
  for (let i = 0; i < noPrefix.length; i += 2) {
    out.push(Number.parseInt(noPrefix.slice(i, i + 2), 16));
  }
  return out;
}

async function parseError(response: Response): Promise<string> {
  let body: ApiErrorBody | null = null;
  try {
    body = (await response.json()) as ApiErrorBody;
  } catch {
    // noop
  }

  if (body?.error?.message) {
    const suffix = [body.error.code, body.requestId].filter(Boolean).join(' / ');
    return suffix ? `${body.error.message} (${suffix})` : body.error.message;
  }

  const fallback = await response.text().catch(() => 'request failed');
  return fallback || 'request failed';
}

async function requestJson<T>(path: string, token: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  headers.set('Content-Type', 'application/json');
  if (token.trim().length > 0) {
    headers.set('Authorization', `Bearer ${token.trim()}`);
  }

  const response = await fetch(path, { ...init, headers });
  if (!response.ok) {
    throw new Error(await parseError(response));
  }

  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.includes('application/json')) {
    return (await response.json()) as T;
  }
  return {} as T;
}

export async function fetchTemplates(token: string): Promise<TemplatePack[]> {
  const response = await requestJson<{ templatePacks: TemplatePack[] }>('/templates', token);
  return response.templatePacks;
}

export async function fetchInstances(token: string): Promise<InstanceView[]> {
  const response = await requestJson<{ instances: InstanceView[] }>('/instances', token);
  return response.instances;
}

export async function createSessionFromAccessToken(
  token: string,
  payload: { instanceId: string; accessToken: string },
): Promise<ApiSessionToken> {
  return requestJson<ApiSessionToken>('/auth/session/token', token, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function requestWalletChallenge(
  token: string,
  payload: { instanceId: string; walletAddress: string },
): Promise<{ challengeId: string; message: string; expiresAt: number }> {
  return requestJson('/auth/challenge', token, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function verifyWalletSession(
  token: string,
  payload: { challengeId: string; signature: string },
): Promise<ApiSessionToken> {
  return requestJson<ApiSessionToken>('/auth/session/wallet', token, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function startSetup(
  token: string,
  instanceId: string,
  env: Record<string, string>,
): Promise<InstanceView> {
  return requestJson<InstanceView>(`/instances/${encodeURIComponent(instanceId)}/setup/start`, token, {
    method: 'POST',
    body: JSON.stringify({ env }),
  });
}

export async function getInstanceAccess(token: string, instanceId: string): Promise<InstanceAccess> {
  return requestJson<InstanceAccess>(`/instances/${encodeURIComponent(instanceId)}/access`, token);
}

export async function runTerminalCommand(
  token: string,
  instanceId: string,
  command: string,
): Promise<ExecuteTerminalResponse> {
  const create = await requestJson<{ data: { sessionId: string } }>(
    `/instances/${encodeURIComponent(instanceId)}/terminals`,
    token,
    { method: 'POST' },
  );

  const sessionId = create.data?.sessionId;
  if (!sessionId) {
    throw new Error('terminal session id missing');
  }

  try {
    return await requestJson<ExecuteTerminalResponse>(
      `/instances/${encodeURIComponent(instanceId)}/terminals/${encodeURIComponent(sessionId)}/execute`,
      token,
      {
        method: 'POST',
        body: JSON.stringify({ command }),
      },
    );
  } finally {
    await requestJson(
      `/instances/${encodeURIComponent(instanceId)}/terminals/${encodeURIComponent(sessionId)}`,
      token,
      { method: 'DELETE' },
    ).catch(() => undefined);
  }
}

export async function updateSshKey(
  token: string,
  instanceId: string,
  payload: { username: string; publicKey: string },
  method: 'POST' | 'DELETE',
): Promise<void> {
  await requestJson(`/instances/${encodeURIComponent(instanceId)}/ssh`, token, {
    method,
    body: JSON.stringify(payload),
  });
}

export async function teePublicKey(token: string, instanceId: string): Promise<TeePublicKeyResponse> {
  return requestJson<TeePublicKeyResponse>(`/instances/${encodeURIComponent(instanceId)}/tee/public-key`, token);
}

export async function teeAttestation(token: string, instanceId: string): Promise<TeeAttestationResponse> {
  return requestJson<TeeAttestationResponse>(`/instances/${encodeURIComponent(instanceId)}/tee/attestation`, token);
}

export async function teeSealedSecrets(
  token: string,
  instanceId: string,
  payload: { algorithm: string; ciphertext: number[]; nonce: number[] },
): Promise<TeeSealedSecretsResponse> {
  return requestJson<TeeSealedSecretsResponse>(
    `/instances/${encodeURIComponent(instanceId)}/tee/sealed-secrets`,
    token,
    {
      method: 'POST',
      body: JSON.stringify({ sealedSecret: payload }),
    },
  );
}

export async function createChatSession(
  token: string,
  instanceId: string,
  title: string,
): Promise<SessionSummary> {
  return requestJson<SessionSummary>(`/instances/${encodeURIComponent(instanceId)}/session/sessions`, token, {
    method: 'POST',
    body: JSON.stringify({ title }),
  });
}

export async function sendChatMessage(
  token: string,
  instanceId: string,
  sessionId: string,
  prompt: string,
): Promise<void> {
  await requestJson(
    `/instances/${encodeURIComponent(instanceId)}/session/sessions/${encodeURIComponent(sessionId)}/messages`,
    token,
    {
      method: 'POST',
      body: JSON.stringify({ parts: [{ type: 'text', text: prompt }] }),
    },
  );
}

export async function getSessionMessages(
  token: string,
  instanceId: string,
  sessionId: string,
): Promise<Array<{ info?: { role?: string }; parts?: Array<{ text?: string }> }>> {
  return requestJson(
    `/instances/${encodeURIComponent(instanceId)}/session/sessions/${encodeURIComponent(sessionId)}/messages?limit=20`,
    token,
  );
}

export function parseEnvText(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of raw.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf('=');
    if (idx <= 0) {
      throw new Error(`Invalid env line: ${trimmed}`);
    }
    out[trimmed.slice(0, idx).trim()] = trimmed.slice(idx + 1);
  }
  return out;
}
