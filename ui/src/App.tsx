import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import {
  useCreateSession,
  useDeleteSession,
  useRenameSession,
  useSessionStream,
  useSessions,
  ChatContainer,
  type AgentBranding,
} from '@tangle-network/agent-ui';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  AnimatedPage,
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Input,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Textarea,
} from '@tangle-network/blueprint-ui/components';
import { cn } from '@tangle-network/blueprint-ui';
import {
  createSessionFromAccessToken,
  createChatSession,
  fetchInstances,
  fetchTemplates,
  getInstanceAccess,
  getSessionMessages,
  loadSavedToken,
  parseByteSequence,
  parseEnvText,
  requestWalletChallenge,
  runTerminalCommand,
  saveToken,
  sendChatMessage,
  startSetup,
  teeAttestation,
  teePublicKey,
  teeSealedSecrets,
  updateSshKey,
  verifyWalletSession,
  type InstanceAccess,
  type InstanceView,
  type TemplatePack,
} from '~/lib/api';

const TerminalView = lazy(() => import('@tangle-network/agent-ui/terminal').then((m) => ({ default: m.TerminalView })));

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      refetchOnWindowFocus: false,
    },
  },
});

const CHAT_BRANDING: AgentBranding = {
  label: 'Claw Runtime',
  accentClass: 'text-teal-300',
  bgClass: 'bg-teal-500/8',
  borderClass: 'border-teal-400/20',
  containerBgClass: 'bg-[#0f1824]/80',
  textClass: 'text-teal-200',
  iconClass: 'i-ph:robot',
};

type NoticeTone = 'success' | 'error' | 'info';

type MainTab = 'workspace' | 'terminal' | 'chat' | 'advanced';

function toneClasses(tone: NoticeTone): string {
  if (tone === 'success') return 'border-teal-400/20 bg-teal-500/8 text-teal-200';
  if (tone === 'error') return 'border-rose-400/25 bg-rose-500/10 text-rose-200';
  return 'border-sky-400/20 bg-sky-500/8 text-sky-200';
}

function statusTone(status: string): 'success' | 'amber' | 'destructive' | 'secondary' {
  if (status === 'running') return 'success';
  if (status === 'creating' || status === 'pending') return 'amber';
  if (status === 'deleted' || status === 'error') return 'destructive';
  return 'secondary';
}

function formatDate(value: number): string {
  if (!Number.isFinite(value)) return 'n/a';
  return new Date(value * 1000).toLocaleString();
}

function firstAssistantReply(
  messages: Array<{ info?: { role?: string }; parts?: Array<{ text?: string }> }>,
): string {
  const match = [...messages].reverse().find((item) => item.info?.role === 'assistant');
  return match?.parts?.map((part) => part.text ?? '').join('\n').trim() || 'No assistant response yet.';
}

function InstanceRuntimePanel() {
  const [token, setToken] = useState('');
  const [tokenInput, setTokenInput] = useState('');
  const [notice, setNotice] = useState<{ tone: NoticeTone; text: string } | null>(null);
  const [templates, setTemplates] = useState<TemplatePack[]>([]);
  const [instances, setInstances] = useState<InstanceView[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [selectedId, setSelectedId] = useState<string>('');
  const [tab, setTab] = useState<MainTab>('workspace');

  const [instanceAccess, setInstanceAccess] = useState<InstanceAccess | null>(null);
  const [setupEnvText, setSetupEnvText] = useState('');
  const [terminalCommand, setTerminalCommand] = useState('echo "OpenClaw runtime is ready"');
  const [terminalOutput, setTerminalOutput] = useState<string>('');
  const [sshUsername, setSshUsername] = useState('agent');
  const [sshPublicKey, setSshPublicKey] = useState('');
  const [teeAlgorithm, setTeeAlgorithm] = useState('x25519-xsalsa20-poly1305');
  const [teeCiphertext, setTeeCiphertext] = useState('');
  const [teeNonce, setTeeNonce] = useState('');
  const [teeOutput, setTeeOutput] = useState('');

  const [accessInstanceId, setAccessInstanceId] = useState('');
  const [accessTokenInput, setAccessTokenInput] = useState('');
  const [walletInstanceId, setWalletInstanceId] = useState('');
  const [walletAddress, setWalletAddress] = useState('');
  const [challengeId, setChallengeId] = useState('');
  const [challengeMessage, setChallengeMessage] = useState('');
  const [walletSignature, setWalletSignature] = useState('');
  const [quickChatPrompt, setQuickChatPrompt] = useState('hello');

  useEffect(() => {
    const saved = loadSavedToken();
    setToken(saved);
    setTokenInput(saved);
  }, []);

  const selectedInstance = useMemo(
    () => instances.find((instance) => instance.id === selectedId) ?? null,
    [instances, selectedId],
  );

  const selectedApiBase = selectedInstance
    ? `/instances/${encodeURIComponent(selectedInstance.id)}`
    : '';

  const sessions = useSessions(selectedApiBase, token || null);
  const createSessionMutation = useCreateSession(selectedApiBase, token || null);
  const deleteSessionMutation = useDeleteSession(selectedApiBase, token || null);
  const renameSessionMutation = useRenameSession(selectedApiBase, token || null);
  const [activeSessionId, setActiveSessionId] = useState('');

  const sessionStream = useSessionStream({
    apiUrl: selectedApiBase,
    token: token || null,
    sessionId: activeSessionId,
    enabled: Boolean(selectedApiBase && token && activeSessionId),
  });

  useEffect(() => {
    const first = sessions.data?.[0]?.id ?? '';
    if (!first) {
      setActiveSessionId('');
      return;
    }
    setActiveSessionId((current) => (current ? current : first));
  }, [sessions.data]);

  useEffect(() => {
    setInstanceAccess(null);
    setTab('workspace');
    setTerminalOutput('');
    setTeeOutput('');
  }, [selectedId]);

  const refresh = useCallback(async () => {
    if (!token.trim()) {
      setTemplates([]);
      setInstances([]);
      return;
    }

    setIsRefreshing(true);
    try {
      const [templatePacks, discoveredInstances] = await Promise.all([
        fetchTemplates(token),
        fetchInstances(token),
      ]);
      setTemplates(templatePacks);
      setInstances(discoveredInstances);
      setSelectedId((current) => {
        if (current && discoveredInstances.some((item) => item.id === current)) {
          return current;
        }
        return discoveredInstances[0]?.id ?? '';
      });
      setNotice({ tone: 'success', text: 'Control plane state refreshed.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `Refresh failed: ${(error as Error).message}` });
    } finally {
      setIsRefreshing(false);
    }
  }, [token]);

  useEffect(() => {
    if (!token.trim()) return;
    void refresh();
  }, [token, refresh]);

  const applySessionToken = useCallback((nextToken: string, message: string) => {
    saveToken(nextToken);
    setToken(nextToken);
    setTokenInput(nextToken);
    setNotice({ tone: 'success', text: message });
  }, []);

  const onSaveToken = useCallback(() => {
    const normalized = tokenInput.trim();
    saveToken(normalized);
    setToken(normalized);
    setNotice({ tone: 'success', text: normalized ? 'Bearer token saved.' : 'Bearer token cleared.' });
  }, [tokenInput]);

  const onAccessTokenLogin = useCallback(async () => {
    try {
      const session = await createSessionFromAccessToken(token, {
        instanceId: accessInstanceId.trim(),
        accessToken: accessTokenInput.trim(),
      });
      applySessionToken(session.token, 'Owner session created from access token.');
      await refresh();
    } catch (error) {
      setNotice({ tone: 'error', text: `Access-token login failed: ${(error as Error).message}` });
    }
  }, [accessInstanceId, accessTokenInput, applySessionToken, refresh, token]);

  const onWalletChallenge = useCallback(async () => {
    try {
      const response = await requestWalletChallenge(token, {
        instanceId: walletInstanceId.trim(),
        walletAddress: walletAddress.trim(),
      });
      setChallengeId(response.challengeId);
      setChallengeMessage(response.message);
      setNotice({ tone: 'info', text: 'Challenge generated. Sign the message and verify.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `Challenge creation failed: ${(error as Error).message}` });
    }
  }, [token, walletAddress, walletInstanceId]);

  const onWalletVerify = useCallback(async () => {
    try {
      const session = await verifyWalletSession(token, {
        challengeId: challengeId.trim(),
        signature: walletSignature.trim(),
      });
      applySessionToken(session.token, 'Wallet session verified and saved.');
      await refresh();
    } catch (error) {
      setNotice({ tone: 'error', text: `Wallet verification failed: ${(error as Error).message}` });
    }
  }, [applySessionToken, challengeId, refresh, token, walletSignature]);

  const onOneClickSetup = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const updated = await startSetup(token, selectedInstance.id, {});
      setInstances((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setNotice({ tone: 'success', text: `Setup started for ${selectedInstance.name}.` });
    } catch (error) {
      setNotice({ tone: 'error', text: `Setup failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, token]);

  const onSetupWithEnv = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const env = parseEnvText(setupEnvText);
      const updated = await startSetup(token, selectedInstance.id, env);
      setInstances((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setNotice({ tone: 'success', text: `Setup started with ${Object.keys(env).length} env override(s).` });
    } catch (error) {
      setNotice({ tone: 'error', text: `Advanced setup failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, setupEnvText, token]);

  const onFetchInstanceAccess = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const access = await getInstanceAccess(token, selectedInstance.id);
      setInstanceAccess(access);
      setNotice({ tone: 'success', text: 'Instance access credentials retrieved.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `Access retrieval failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, token]);

  const onRunTerminalCommand = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const output = await runTerminalCommand(token, selectedInstance.id, terminalCommand);
      const text = [
        `Exit: ${output.exitCode}`,
        '',
        'STDOUT:',
        output.stdout || '(empty)',
        '',
        'STDERR:',
        output.stderr || '(empty)',
      ].join('\n');
      setTerminalOutput(text);
      setNotice({ tone: 'success', text: 'One-shot terminal command completed.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `Terminal command failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, terminalCommand, token]);

  const onQuickChat = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const session = await createChatSession(token, selectedInstance.id, 'Quick prompt');
      await sendChatMessage(token, selectedInstance.id, session.id, quickChatPrompt);
      const messages = await getSessionMessages(token, selectedInstance.id, session.id);
      setNotice({ tone: 'info', text: `Assistant reply: ${firstAssistantReply(messages)}` });
    } catch (error) {
      setNotice({ tone: 'error', text: `Quick chat failed: ${(error as Error).message}` });
    }
  }, [quickChatPrompt, selectedInstance, token]);

  const onSshUpsert = useCallback(async (method: 'POST' | 'DELETE') => {
    if (!selectedInstance) return;
    try {
      await updateSshKey(
        token,
        selectedInstance.id,
        { username: sshUsername.trim(), publicKey: sshPublicKey.trim() },
        method,
      );
      setNotice({
        tone: 'success',
        text: method === 'POST' ? 'SSH key provisioned.' : 'SSH key revoked.',
      });
    } catch (error) {
      setNotice({ tone: 'error', text: `SSH update failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, sshPublicKey, sshUsername, token]);

  const onTeePublicKey = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const payload = await teePublicKey(token, selectedInstance.id);
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE public key fetched.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE public key failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, token]);

  const onTeeAttestation = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const payload = await teeAttestation(token, selectedInstance.id);
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE attestation fetched.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE attestation failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, token]);

  const onTeeSealedSecret = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const payload = await teeSealedSecrets(token, selectedInstance.id, {
        algorithm: teeAlgorithm,
        ciphertext: parseByteSequence(teeCiphertext),
        nonce: parseByteSequence(teeNonce),
      });
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE sealed secret payload submitted.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE sealed secret failed: ${(error as Error).message}` });
    }
  }, [selectedInstance, teeAlgorithm, teeCiphertext, teeNonce, token]);

  return (
    <div className="min-h-screen bg-claw-elements-background-depth-1 text-claw-elements-textPrimary bg-mesh relative">
      <header className="sticky top-0 z-20 border-b border-claw-elements-dividerColor glass-strong">
        <div className="mx-auto max-w-7xl px-4 py-3 sm:px-6 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <div className="i-ph:cube text-teal-300 text-lg" />
              <h1 className="font-display text-base sm:text-lg truncate">OpenClaw Control Plane</h1>
            </div>
            <p className="text-xs sm:text-sm text-claw-elements-textSecondary">
              One-click runtime onboarding for OpenClaw, NanoClaw, and IronClaw.
            </p>
          </div>
          <Button size="sm" variant="secondary" disabled={isRefreshing || !token} onClick={() => void refresh()}>
            {isRefreshing ? 'Refreshing...' : 'Refresh'}
          </Button>
        </div>
      </header>

      <AnimatedPage className="mx-auto max-w-7xl px-4 py-6 sm:px-6 relative z-1">
        {notice ? (
          <div className={cn('mb-4 rounded-lg border px-3 py-2 text-sm', toneClasses(notice.tone))}>{notice.text}</div>
        ) : null}

        <div className="grid gap-6 lg:grid-cols-[360px_minmax(0,1fr)]">
          <div className="space-y-6">
            <Card className="glass">
              <CardHeader>
                <CardTitle className="text-sm">Session</CardTitle>
                <CardDescription>Use one bearer token, then refresh and manage instances.</CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <Input
                  type="password"
                  value={tokenInput}
                  onChange={(event) => setTokenInput(event.target.value)}
                  placeholder="oclw_..."
                />
                <Button className="w-full" onClick={onSaveToken}>Save Bearer Token</Button>
                <details className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                  <summary className="cursor-pointer text-xs text-claw-elements-textSecondary">
                    Advanced auth options
                  </summary>
                  <div className="mt-3 space-y-4 text-sm">
                    <div className="space-y-2">
                      <p className="text-xs font-display uppercase tracking-wide text-claw-elements-textTertiary">
                        Access-token login
                      </p>
                      <Input
                        value={accessInstanceId}
                        onChange={(event) => setAccessInstanceId(event.target.value)}
                        placeholder="instance-id"
                      />
                      <Input
                        type="password"
                        value={accessTokenInput}
                        onChange={(event) => setAccessTokenInput(event.target.value)}
                        placeholder="instance access token"
                      />
                      <Button
                        variant="secondary"
                        className="w-full"
                        onClick={() => void onAccessTokenLogin()}
                        disabled={!accessInstanceId.trim() || !accessTokenInput.trim()}
                      >
                        Create Owner Session
                      </Button>
                    </div>

                    <div className="space-y-2 border-t border-claw-elements-dividerColor pt-3">
                      <p className="text-xs font-display uppercase tracking-wide text-claw-elements-textTertiary">
                        Wallet login
                      </p>
                      <Input
                        value={walletInstanceId}
                        onChange={(event) => setWalletInstanceId(event.target.value)}
                        placeholder="instance-id"
                      />
                      <Input
                        value={walletAddress}
                        onChange={(event) => setWalletAddress(event.target.value)}
                        placeholder="0x..."
                      />
                      <Button
                        variant="secondary"
                        className="w-full"
                        onClick={() => void onWalletChallenge()}
                        disabled={!walletInstanceId.trim() || !walletAddress.trim()}
                      >
                        Get Challenge
                      </Button>
                      <Textarea
                        readOnly
                        value={challengeMessage}
                        className="min-h-18"
                        placeholder="Challenge message appears here"
                      />
                      <Input
                        value={challengeId}
                        onChange={(event) => setChallengeId(event.target.value)}
                        placeholder="challenge-id"
                      />
                      <Input
                        value={walletSignature}
                        onChange={(event) => setWalletSignature(event.target.value)}
                        placeholder="0x signature"
                      />
                      <Button
                        variant="secondary"
                        className="w-full"
                        onClick={() => void onWalletVerify()}
                        disabled={!challengeId.trim() || !walletSignature.trim()}
                      >
                        Verify Wallet Session
                      </Button>
                    </div>
                  </div>
                </details>
              </CardContent>
            </Card>

            <Card className="glass">
              <CardHeader>
                <CardTitle className="text-sm">Templates</CardTitle>
                <CardDescription>Runtime presets available on this operator.</CardDescription>
              </CardHeader>
              <CardContent className="space-y-2">
                {templates.length === 0 ? (
                  <p className="text-sm text-claw-elements-textTertiary">No templates loaded.</p>
                ) : (
                  templates.map((pack) => (
                    <div key={pack.id} className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                      <div className="flex items-center justify-between gap-2">
                        <p className="font-display text-sm">{pack.name}</p>
                        <Badge variant="secondary">{pack.mode}</Badge>
                      </div>
                      <p className="mt-1 text-xs text-claw-elements-textSecondary">{pack.description}</p>
                    </div>
                  ))
                )}
              </CardContent>
            </Card>
          </div>

          <div className="space-y-6 min-w-0">
            <Card className="glass">
              <CardHeader>
                <CardTitle className="text-sm">Instances</CardTitle>
                <CardDescription>
                  Select an instance to start setup and work in terminal/chat. Lifecycle jobs remain on-chain.
                </CardDescription>
              </CardHeader>
              <CardContent>
                {instances.length === 0 ? (
                  <p className="text-sm text-claw-elements-textTertiary">No instances visible for this session.</p>
                ) : (
                  <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                    {instances.map((instance) => (
                      <button
                        key={instance.id}
                        type="button"
                        onClick={() => setSelectedId(instance.id)}
                        className={cn(
                          'text-left rounded-xl border px-3 py-3 transition-colors',
                          selectedId === instance.id
                            ? 'border-teal-300/40 bg-teal-500/10'
                            : 'border-claw-elements-dividerColor hover:bg-claw-elements-item-backgroundHover',
                        )}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <p className="font-display text-sm truncate">{instance.name}</p>
                          <Badge variant={statusTone(instance.status)}>{instance.status}</Badge>
                        </div>
                        <p className="mt-2 text-xs text-claw-elements-textSecondary">
                          {instance.clawVariant} · {instance.executionTarget}
                        </p>
                        <p className="text-[11px] text-claw-elements-textTertiary font-data mt-1 truncate">
                          {instance.id}
                        </p>
                      </button>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>

            {selectedInstance ? (
              <Card className="glass">
                <CardHeader>
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <CardTitle className="text-base">{selectedInstance.name}</CardTitle>
                      <CardDescription>
                        Owner {selectedInstance.owner} · created {formatDate(selectedInstance.createdAt)}
                      </CardDescription>
                    </div>
                    <div className="flex items-center gap-2">
                      <Badge variant={statusTone(selectedInstance.status)}>{selectedInstance.status}</Badge>
                      <Badge variant="secondary">{selectedInstance.clawVariant}</Badge>
                      {selectedInstance.executionTarget === 'tee' ? <Badge variant="amber">TEE</Badge> : null}
                    </div>
                  </div>
                </CardHeader>

                <CardContent>
                  <Tabs value={tab} onValueChange={(value) => setTab(value as MainTab)}>
                    <TabsList className="grid grid-cols-2 sm:grid-cols-4 gap-1 mb-4">
                      <TabsTrigger value="workspace">Workspace</TabsTrigger>
                      <TabsTrigger value="terminal">Terminal</TabsTrigger>
                      <TabsTrigger value="chat">Chat</TabsTrigger>
                      <TabsTrigger value="advanced">Advanced</TabsTrigger>
                    </TabsList>

                    <TabsContent value="workspace" className="space-y-4">
                      <div className="grid gap-4 xl:grid-cols-2">
                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">One-click setup</CardTitle>
                            <CardDescription>Starts the variant bootstrap using defaults.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Button onClick={() => void onOneClickSetup()} disabled={selectedInstance.status !== 'running'}>
                              Start Setup
                            </Button>
                            <div className="text-xs text-claw-elements-textSecondary space-y-1">
                              <p>Runtime setup status: {selectedInstance.runtime.setupStatus ?? 'n/a'}</p>
                              <p>Setup URL: {selectedInstance.runtime.setupUrl ?? 'n/a'}</p>
                              <p>Tunnel: {selectedInstance.uiAccess.tunnelStatus}</p>
                            </div>
                          </CardContent>
                        </Card>

                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">Instance access</CardTitle>
                            <CardDescription>Owner-scoped UI bearer for runtime ingress.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Button variant="secondary" onClick={() => void onFetchInstanceAccess()}>
                              Get Access Credentials
                            </Button>
                            {instanceAccess ? (
                              <div className="space-y-2 text-xs font-data">
                                <div>
                                  <p className="text-claw-elements-textTertiary">Bearer token</p>
                                  <p className="break-all">{instanceAccess.bearerToken}</p>
                                </div>
                                <div>
                                  <p className="text-claw-elements-textTertiary">Public URL</p>
                                  <p>{instanceAccess.publicUrl ?? 'n/a'}</p>
                                </div>
                                <div>
                                  <p className="text-claw-elements-textTertiary">Local URL</p>
                                  <p>{instanceAccess.uiLocalUrl ?? 'n/a'}</p>
                                </div>
                              </div>
                            ) : null}
                          </CardContent>
                        </Card>
                      </div>
                    </TabsContent>

                    <TabsContent value="terminal">
                      <div className="h-[min(560px,72vh)] rounded-xl border border-claw-elements-dividerColor overflow-hidden bg-[#070d15]">
                        {token ? (
                          <Suspense fallback={<div className="p-4 text-sm">Loading terminal...</div>}>
                            <TerminalView apiUrl={selectedApiBase} token={token} title="OpenClaw Terminal" subtitle="Scoped runtime shell" />
                          </Suspense>
                        ) : (
                          <div className="p-6 text-sm text-claw-elements-textSecondary">
                            Save a bearer token first to open terminal access.
                          </div>
                        )}
                      </div>
                    </TabsContent>

                    <TabsContent value="chat" className="space-y-4">
                      <div className="grid gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">Sessions</CardTitle>
                            <CardDescription>Instance-scoped live sessions.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-2 max-h-[520px] overflow-y-auto scroll-thin">
                            <Button
                              variant="secondary"
                              className="w-full"
                              onClick={() =>
                                createSessionMutation.mutate('Session', {
                                  onSuccess: (session) => {
                                    setActiveSessionId(session.id);
                                    setNotice({ tone: 'success', text: 'New chat session created.' });
                                  },
                                  onError: (error) => {
                                    setNotice({ tone: 'error', text: `Session create failed: ${(error as Error).message}` });
                                  },
                                })
                              }
                              disabled={!selectedApiBase || !token || createSessionMutation.isPending}
                            >
                              {createSessionMutation.isPending ? 'Creating...' : 'New Session'}
                            </Button>

                            {(sessions.data ?? []).map((session) => (
                              <div key={session.id} className="rounded-lg border border-claw-elements-dividerColor p-2">
                                <button
                                  type="button"
                                  className={cn(
                                    'w-full text-left text-sm truncate',
                                    activeSessionId === session.id ? 'text-teal-200' : 'text-claw-elements-textPrimary',
                                  )}
                                  onClick={() => setActiveSessionId(session.id)}
                                >
                                  {session.title}
                                </button>
                                <div className="mt-2 flex gap-2">
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    className="h-7 px-2"
                                    onClick={() => {
                                      const title = prompt('Rename session', session.title);
                                      if (!title || !title.trim()) return;
                                      renameSessionMutation.mutate(
                                        { sessionId: session.id, title: title.trim() },
                                        {
                                          onError: (error) => {
                                            setNotice({ tone: 'error', text: `Rename failed: ${(error as Error).message}` });
                                          },
                                        },
                                      );
                                    }}
                                  >
                                    Rename
                                  </Button>
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    className="h-7 px-2 text-rose-300 hover:text-rose-200"
                                    onClick={() =>
                                      deleteSessionMutation.mutate(session.id, {
                                        onSuccess: () => {
                                          setActiveSessionId('');
                                          setNotice({ tone: 'info', text: 'Session deleted.' });
                                        },
                                        onError: (error) => {
                                          setNotice({ tone: 'error', text: `Delete failed: ${(error as Error).message}` });
                                        },
                                      })
                                    }
                                  >
                                    Delete
                                  </Button>
                                </div>
                              </div>
                            ))}
                          </CardContent>
                        </Card>

                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2 min-h-[520px]">
                          <CardContent className="p-0 h-full min-h-[520px]">
                            {activeSessionId ? (
                              <ChatContainer
                                messages={sessionStream.messages}
                                partMap={sessionStream.partMap}
                                isStreaming={sessionStream.isStreaming}
                                onSend={(text) => {
                                  void sessionStream.send(text);
                                }}
                                branding={CHAT_BRANDING}
                                placeholder="Ask your runtime to run setup or execute commands..."
                                className="h-[520px]"
                              />
                            ) : (
                              <div className="h-[520px] p-5 text-sm text-claw-elements-textSecondary">
                                Create a session to start chat.
                              </div>
                            )}
                          </CardContent>
                        </Card>
                      </div>
                    </TabsContent>

                    <TabsContent value="advanced" className="space-y-4">
                      <div className="grid gap-4 xl:grid-cols-2">
                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">Setup Env Overrides</CardTitle>
                            <CardDescription>Optional KEY=VALUE lines for setup bootstrap.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Textarea
                              value={setupEnvText}
                              onChange={(event) => setSetupEnvText(event.target.value)}
                              placeholder={'OPENCLAW_THEME=night\nOPENCLAW_REGION=us-west'}
                              className="min-h-32 font-data"
                            />
                            <Button variant="secondary" onClick={() => void onSetupWithEnv()}>
                              Start Setup With Env
                            </Button>
                          </CardContent>
                        </Card>

                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">One-shot command</CardTitle>
                            <CardDescription>Run a command with immediate stdout/stderr capture.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Input
                              value={terminalCommand}
                              onChange={(event) => setTerminalCommand(event.target.value)}
                              placeholder="echo hello"
                              className="font-data"
                            />
                            <Button variant="secondary" onClick={() => void onRunTerminalCommand()}>
                              Run Command
                            </Button>
                            <Textarea readOnly value={terminalOutput} className="min-h-32 font-data" />
                          </CardContent>
                        </Card>
                      </div>

                      <div className="grid gap-4 xl:grid-cols-2">
                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">SSH key management</CardTitle>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Input
                              value={sshUsername}
                              onChange={(event) => setSshUsername(event.target.value)}
                              placeholder="agent"
                            />
                            <Textarea
                              value={sshPublicKey}
                              onChange={(event) => setSshPublicKey(event.target.value)}
                              placeholder="ssh-ed25519 AAAA..."
                              className="min-h-22 font-data"
                            />
                            <div className="flex gap-2">
                              <Button variant="secondary" onClick={() => void onSshUpsert('POST')}>Add Key</Button>
                              <Button variant="ghost" onClick={() => void onSshUpsert('DELETE')}>Revoke Key</Button>
                            </div>
                          </CardContent>
                        </Card>

                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">Quick assistant prompt</CardTitle>
                            <CardDescription>Single request for smoke testing runtime chat.</CardDescription>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <Input
                              value={quickChatPrompt}
                              onChange={(event) => setQuickChatPrompt(event.target.value)}
                              placeholder="hello"
                            />
                            <Button variant="secondary" onClick={() => void onQuickChat()}>
                              Send Quick Prompt
                            </Button>
                          </CardContent>
                        </Card>
                      </div>

                      {selectedInstance.executionTarget === 'tee' ? (
                        <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                          <CardHeader>
                            <CardTitle className="text-sm">TEE controls</CardTitle>
                          </CardHeader>
                          <CardContent className="space-y-3">
                            <div className="grid gap-3 sm:grid-cols-3">
                              <Button variant="secondary" onClick={() => void onTeePublicKey()}>Public Key</Button>
                              <Button variant="secondary" onClick={() => void onTeeAttestation()}>Attestation</Button>
                              <Button variant="secondary" onClick={() => void onTeeSealedSecret()}>Sealed Secret</Button>
                            </div>
                            <Input
                              value={teeAlgorithm}
                              onChange={(event) => setTeeAlgorithm(event.target.value)}
                              placeholder="x25519-xsalsa20-poly1305"
                            />
                            <Textarea
                              value={teeCiphertext}
                              onChange={(event) => setTeeCiphertext(event.target.value)}
                              placeholder="ciphertext bytes as [1,2,3] or 0x010203"
                              className="min-h-20 font-data"
                            />
                            <Textarea
                              value={teeNonce}
                              onChange={(event) => setTeeNonce(event.target.value)}
                              placeholder="nonce bytes as [1,2,3] or 0x010203"
                              className="min-h-20 font-data"
                            />
                            <Textarea readOnly value={teeOutput} className="min-h-40 font-data" />
                          </CardContent>
                        </Card>
                      ) : null}
                    </TabsContent>
                  </Tabs>
                </CardContent>
              </Card>
            ) : null}
          </div>
        </div>
      </AnimatedPage>
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <InstanceRuntimePanel />
    </QueryClientProvider>
  );
}
