import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  useCreateSession,
  useDeleteSession,
  useRenameSession,
  useSessionStream,
  useSessions,
  ChatContainer,
  copyText,
  truncateAddress,
  type AgentBranding,
} from '@tangle-network/agent-ui';
import { selectedChainIdStore, useSubmitJob } from '@tangle-network/blueprint-ui';
import {
  AnimatedPage,
  AppToaster,
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
import { encodeAbiParameters, formatUnits, isAddress } from 'viem';
import { useAccount, useBalance, useConnect, useDisconnect, useSignMessage, useSwitchChain } from 'wagmi';
import { toast } from 'sonner';
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
  provisionInstance,
  runTerminalCommand,
  saveToken,
  sendChatMessage,
  startSetup,
  requestWalletChallenge,
  teeAttestation,
  teePublicKey,
  teeSealedSecrets,
  updateSshKey,
  verifyWalletSession,
  type InstanceAccess,
  type InstanceView,
  type TemplatePack,
} from '~/lib/api';
import openclawArt from '~/assets/variants/openclaw.svg';
import nanoclawArt from '~/assets/variants/nanoclaw.svg';
import ironclawArt from '~/assets/variants/ironclaw.svg';

const TerminalView = lazy(() => import('@tangle-network/agent-ui/terminal').then((m) => ({ default: m.TerminalView })));
const DEMO_TOKEN = import.meta.env.VITE_OPERATOR_API_TOKEN ?? 'oclw_dev_operator_token';
const DEV_INSTANCE_ACCESS_TOKEN = (import.meta.env.VITE_UI_ACCESS_TOKEN ?? '').trim();
const DEMO_MODE = import.meta.env.VITE_DEMO_MODE === '1';

const JOB_CREATE = 0;
const JOB_START = 1;
const JOB_STOP = 2;
const JOB_DELETE = 3;
const TARGET_CHAIN_ID = Number(import.meta.env.VITE_CHAIN_ID ?? 31337);
const FALLBACK_TARGET_RPC_URL =
  typeof window !== 'undefined'
    ? `${window.location.protocol === 'https:' ? 'https' : 'http'}://${window.location.hostname}:8745`
    : 'http://127.0.0.1:8745';
const TARGET_RPC_URL = import.meta.env.VITE_RPC_URL ?? FALLBACK_TARGET_RPC_URL;
const TARGET_CHAIN_NAME = import.meta.env.VITE_CHAIN_NAME ?? 'Tangle Local';
const TARGET_CURRENCY_SYMBOL = import.meta.env.VITE_CHAIN_CURRENCY_SYMBOL ?? 'ETH';
const TARGET_EXPLORER_URL = import.meta.env.VITE_CHAIN_EXPLORER_URL ?? '';
const STRICT_WALLET_RPC_MATCH = import.meta.env.VITE_STRICT_WALLET_RPC_MATCH === '1';

type BrowserEthereum = {
  request: (args: { method: string; params?: unknown[] }) => Promise<unknown>;
};

function browserEthereum(): BrowserEthereum | null {
  const candidate = (globalThis as { ethereum?: BrowserEthereum }).ethereum;
  return candidate?.request ? candidate : null;
}

function parseServiceId(raw: string | undefined): bigint | null {
  if (!raw || !raw.trim()) return null;
  try {
    const value = BigInt(raw.trim());
    return value >= 0n ? value : null;
  } catch {
    return null;
  }
}

const DEFAULT_STANDARD_SERVICE_ID =
  parseServiceId(import.meta.env.VITE_INSTANCE_SERVICE_ID ?? import.meta.env.VITE_SERVICE_ID) ??
  parseServiceId(import.meta.env.VITE_TEE_INSTANCE_SERVICE_ID);
const DEFAULT_TEE_SERVICE_ID =
  parseServiceId(import.meta.env.VITE_TEE_INSTANCE_SERVICE_ID ?? import.meta.env.VITE_SERVICE_ID) ??
  parseServiceId(import.meta.env.VITE_INSTANCE_SERVICE_ID);

const CHAT_BRANDING: AgentBranding = {
  label: 'Claw Runtime',
  accentClass: 'claw-text-accent',
  bgClass: 'bg-teal-500/8',
  borderClass: 'border-teal-400/20',
  containerBgClass: 'bg-[#0f1824]/80',
  textClass: 'claw-text-accent',
  iconClass: 'i-ph:robot',
};

type NoticeTone = 'success' | 'error' | 'info';
type NoticeState = { tone: NoticeTone; text: string };
type ClawVariant = 'openclaw' | 'nanoclaw' | 'ironclaw';
type SurfaceTab = 'launch' | 'instances' | 'workspace';
type WizardStep = 1 | 2 | 3;
type SessionSource = 'wallet_signature' | 'access_token';

type MainTab = 'workspace' | 'terminal' | 'chat' | 'advanced';
type PendingSessionDelete = { id: string; title: string };
type ScopedSession = {
  token: string;
  expiresAt: number;
  owner: string;
  instanceId: string;
  source: SessionSource;
};

const VARIANT_PRESENTATION: Record<
  ClawVariant,
  {
    subtitle: string;
    bullets: [string, string, string];
    art: string;
    badge: string;
    tone: 'teal' | 'amber' | 'rose';
  }
> = {
  openclaw: {
    subtitle: 'Default OpenClaw runtime.',
    bullets: ['Browser setup available', 'Terminal and chat enabled', 'Good default for most users'],
    art: openclawArt,
    badge: 'Standard',
    tone: 'teal',
  },
  nanoclaw: {
    subtitle: 'NanoClaw runtime with minimal footprint.',
    bullets: ['Terminal-first operation', 'Lower overhead', 'Fast iteration'],
    art: nanoclawArt,
    badge: 'Minimal',
    tone: 'amber',
  },
  ironclaw: {
    subtitle: 'IronClaw runtime with stricter defaults.',
    bullets: ['Hardened profile', 'Tighter controls', 'For sensitive workloads'],
    art: ironclawArt,
    badge: 'Strict',
    tone: 'rose',
  },
};

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

function previewToken(value: string): string {
  if (!value) return 'not set';
  if (value.length <= 10) return value;
  return `${value.slice(0, 6)}...${value.slice(-4)}`;
}

function sameAddress(left?: string | null, right?: string | null): boolean {
  if (!left || !right) return false;
  return left.toLowerCase() === right.toLowerCase();
}

function randomSuffix(length = 6): string {
  const alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789';
  const values = new Uint8Array(length);
  if (typeof globalThis.crypto?.getRandomValues === 'function') {
    globalThis.crypto.getRandomValues(values);
  } else {
    for (let idx = 0; idx < length; idx += 1) {
      values[idx] = Math.floor(Math.random() * 256);
    }
  }

  let out = '';
  for (const value of values) {
    out += alphabet[value % alphabet.length];
  }
  return out;
}

function prettyVariantName(variant: ClawVariant): string {
  if (variant === 'openclaw') return 'OpenClaw';
  if (variant === 'nanoclaw') return 'NanoClaw';
  return 'IronClaw';
}

function prettyTemplateMode(mode: string): string {
  const normalized = (mode ?? '').trim().toLowerCase();
  if (!normalized) return 'Default';
  if (normalized === 'ops') return 'Operations';
  if (normalized === 'dev') return 'Developer';
  if (normalized === 'secure') return 'Secure';
  return normalized[0].toUpperCase() + normalized.slice(1);
}

function generateProvisionIdentity(variant: ClawVariant): { name: string; subdomain: string } {
  const suffix = randomSuffix(7);
  return {
    name: `${variant}-${suffix}`,
    subdomain: `${variant}-${suffix}`,
  };
}

function firstAssistantReply(
  messages: Array<{ info?: { role?: string }; parts?: Array<{ text?: string }> }>,
): string {
  const match = [...messages].reverse().find((item) => item.info?.role === 'assistant');
  return match?.parts?.map((part) => part.text ?? '').join('\n').trim() || 'No assistant response yet.';
}

function isMainTab(value: string | null): value is MainTab {
  return value === 'workspace' || value === 'terminal' || value === 'chat' || value === 'advanced';
}

function isSurfaceTab(value: string | null): value is SurfaceTab {
  return value === 'launch' || value === 'instances' || value === 'workspace';
}

function isWizardStep(value: string | null): value is `${WizardStep}` {
  return value === '1' || value === '2' || value === '3';
}

function parseRpcHexToBigint(value: unknown): bigint | null {
  if (typeof value !== 'string') return null;
  if (!/^0x[0-9a-fA-F]+$/.test(value)) return null;
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out.`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) {
      clearTimeout(timer);
    }
  }
}

function readErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'object' && error !== null) {
    const maybe = error as { shortMessage?: unknown; message?: unknown; details?: unknown };
    if (typeof maybe.shortMessage === 'string' && maybe.shortMessage.trim()) return maybe.shortMessage;
    if (typeof maybe.message === 'string' && maybe.message.trim()) return maybe.message;
    if (typeof maybe.details === 'string' && maybe.details.trim()) return maybe.details;
  }
  return String(error ?? 'unknown error');
}

function readErrorCode(error: unknown): number | null {
  if (typeof error === 'object' && error !== null) {
    const maybe = error as { code?: unknown; cause?: { code?: unknown } };
    if (typeof maybe.code === 'number') return maybe.code;
    if (typeof maybe.cause?.code === 'number') return maybe.cause.code;
  }
  return null;
}

async function signWalletMessage(walletAddress: string, message: string): Promise<string> {
  const ethereum = browserEthereum();
  if (!ethereum) {
    throw new Error('Wallet provider unavailable for signature request.');
  }

  const attempts: Array<() => Promise<unknown>> = [
    () => ethereum.request({ method: 'personal_sign', params: [message, walletAddress] }),
    () => ethereum.request({ method: 'personal_sign', params: [walletAddress, message] }),
    () => ethereum.request({ method: 'eth_sign', params: [walletAddress, message] }),
  ];

  let lastError: Error | null = null;
  for (const attempt of attempts) {
    try {
      const signature = await attempt();
      if (typeof signature === 'string' && signature.trim().length > 0) {
        return signature;
      }
      lastError = new Error('Wallet returned an invalid signature payload.');
    } catch (error) {
      lastError = error as Error;
    }
  }
  throw new Error(`Wallet signature failed: ${lastError?.message ?? 'unknown error'}`);
}

async function jsonRpcCall<T>(rpcUrl: string, method: string, params: unknown[] = []): Promise<T> {
  const response = await fetch(rpcUrl, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: Math.floor(Math.random() * 1_000_000_000),
      method,
      params,
    }),
  });

  const payload = (await response.json()) as { result?: T; error?: { code?: number; message?: string } };
  if (!response.ok) {
    throw new Error(`RPC ${method} failed with HTTP ${response.status}`);
  }
  if (payload.error) {
    throw new Error(payload.error.message || `RPC ${method} failed`);
  }
  return payload.result as T;
}

function InstanceRuntimePanel() {
  const [token, setToken] = useState('');
  const [tokenInput, setTokenInput] = useState('');
  const [scopedSessions, setScopedSessions] = useState<Record<string, ScopedSession>>({});
  const [instanceAccessTokenInput, setInstanceAccessTokenInput] = useState<Record<string, string>>({});
  const [isCreatingScopedSession, setIsCreatingScopedSession] = useState(false);
  const [ownerSessionFeedback, setOwnerSessionFeedback] = useState<{
    tone: NoticeTone;
    text: string;
  } | null>(null);
  const [notice, setNotice] = useState<NoticeState | null>(null);
  const [launchFeedback, setLaunchFeedback] = useState<NoticeState | null>(null);
  const [workspaceFeedback, setWorkspaceFeedback] = useState<NoticeState | null>(null);
  const [templates, setTemplates] = useState<TemplatePack[]>([]);
  const [instances, setInstances] = useState<InstanceView[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [selectedId, setSelectedId] = useState<string>('');
  const [tab, setTab] = useState<MainTab>('workspace');
  const [surfaceTab, setSurfaceTab] = useState<SurfaceTab>('launch');
  const [wizardStep, setWizardStep] = useState<WizardStep>(1);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [walletMenuOpen, setWalletMenuOpen] = useState(false);
  const [walletCopied, setWalletCopied] = useState(false);
  const [pendingSessionDelete, setPendingSessionDelete] = useState<PendingSessionDelete | null>(null);
  const sessionDeleteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastNoticeRef = useRef<{ tone: NoticeTone; text: string } | null>(null);
  const txErrorRef = useRef<string | null>(null);
  const txStatusRef = useRef<string>('idle');
  const walletMenuRef = useRef<HTMLDivElement | null>(null);
  const walletMenuButtonRef = useRef<HTMLButtonElement | null>(null);

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
  const [isSetupStarting, setIsSetupStarting] = useState(false);

  const [quickChatPrompt, setQuickChatPrompt] = useState('hello');
  const [provisionName, setProvisionName] = useState('openclaw-' + randomSuffix(7));
  const [provisionVariant, setProvisionVariant] = useState<ClawVariant>('openclaw');
  const [provisionSubdomain, setProvisionSubdomain] = useState('openclaw-' + randomSuffix(7));
  const [provisionTemplateId, setProvisionTemplateId] = useState('ops');
  const [provisionExecutionTarget, setProvisionExecutionTarget] = useState<'standard' | 'tee'>('standard');
  const [isProvisioning, setIsProvisioning] = useState(false);
  const [standardServiceIdInput, setStandardServiceIdInput] = useState(
    DEFAULT_STANDARD_SERVICE_ID?.toString() ?? '',
  );
  const [teeServiceIdInput, setTeeServiceIdInput] = useState(
    DEFAULT_TEE_SERVICE_ID?.toString() ?? '',
  );
  const hydratedFromUrlRef = useRef(false);

  const { address: connectedWallet, chainId, isConnected: isWalletConnected } = useAccount();
  const { connectAsync, connectors, isPending: isWalletConnectPending } = useConnect();
  const { disconnect } = useDisconnect();
  const { signMessageAsync } = useSignMessage();
  const { switchChainAsync, isPending: isSwitchingChain } = useSwitchChain();
  const {
    data: walletBalance,
    error: walletBalanceError,
    isLoading: isWalletBalanceLoading,
  } = useBalance({
    address: connectedWallet,
    chainId,
  });
  const [walletBalanceRpcHex, setWalletBalanceRpcHex] = useState<string | null>(null);
  const [walletBalanceTargetRpcHex, setWalletBalanceTargetRpcHex] = useState<string | null>(null);
  const [isWalletBalanceRpcLoading, setIsWalletBalanceRpcLoading] = useState(false);
  const [providerChainId, setProviderChainId] = useState<number | null>(null);
  const activeChainId = chainId ?? providerChainId ?? undefined;
  const isWrongChain = isWalletConnected && activeChainId !== TARGET_CHAIN_ID;
  const {
    submitJob,
    reset: resetLifecycleTx,
    status: txStatus,
    error: txError,
    txHash,
  } = useSubmitJob();
  const [pendingTxSinceMs, setPendingTxSinceMs] = useState<number | null>(null);
  const [switchRequestSinceMs, setSwitchRequestSinceMs] = useState<number | null>(null);
  const setLaunchNotice = useCallback((tone: NoticeTone, text: string) => {
    const next = { tone, text };
    setLaunchFeedback(next);
    setNotice(next);
  }, []);
  const setWorkspaceNotice = useCallback((tone: NoticeTone, text: string) => {
    const next = { tone, text };
    setWorkspaceFeedback(next);
    setNotice(next);
  }, []);

  useEffect(() => {
    txErrorRef.current = txError;
  }, [txError]);

  useEffect(() => {
    txStatusRef.current = txStatus;
  }, [txStatus]);

  useEffect(() => {
    if (txStatus === 'pending') {
      setPendingTxSinceMs((current) => current ?? Date.now());
      return;
    }
    if (pendingTxSinceMs !== null) {
      setPendingTxSinceMs(null);
    }
  }, [pendingTxSinceMs, txStatus]);

  useEffect(() => {
    if (isSwitchingChain) {
      setSwitchRequestSinceMs((current) => current ?? Date.now());
      return;
    }
    if (switchRequestSinceMs !== null) {
      setSwitchRequestSinceMs(null);
    }
  }, [isSwitchingChain, switchRequestSinceMs]);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const refreshProviderChainId = async () => {
      if (!isWalletConnected) {
        if (!cancelled) setProviderChainId(null);
        return;
      }
      const ethereum = browserEthereum();
      if (!ethereum) {
        if (!cancelled) setProviderChainId(null);
        return;
      }
      try {
        const value = await withTimeout(
          ethereum.request({ method: 'eth_chainId' }).catch(() => null),
          6_000,
          'eth_chainId',
        );
        if (!cancelled && typeof value === 'string') {
          const parsed = Number.parseInt(value, 16);
          setProviderChainId(Number.isFinite(parsed) ? parsed : null);
        }
      } catch {
        if (!cancelled) setProviderChainId(null);
      }
    };

    void refreshProviderChainId();
    if (isWalletConnected) {
      timer = setInterval(() => {
        void refreshProviderChainId();
      }, 7_500);
    }

    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [isWalletConnected]);

  const forceWalletToTargetChain = useCallback(async () => {
    const hexChainId = `0x${TARGET_CHAIN_ID.toString(16)}`;
    const ethereum = browserEthereum();
    const isHttpRpc = TARGET_RPC_URL.startsWith('http://');
    const addParams = {
      chainId: hexChainId,
      chainName: TARGET_CHAIN_NAME,
      nativeCurrency: {
        name: TARGET_CURRENCY_SYMBOL,
        symbol: TARGET_CURRENCY_SYMBOL,
        decimals: 18,
      },
      rpcUrls: [TARGET_RPC_URL],
      ...(TARGET_EXPLORER_URL ? { blockExplorerUrls: [TARGET_EXPLORER_URL] } : {}),
    };

    const switchThroughWallet = async () => {
      if (!ethereum) return;
      await withTimeout(
        ethereum.request({
          method: 'wallet_switchEthereumChain',
          params: [{ chainId: hexChainId }],
        }),
        12_000,
        'wallet_switchEthereumChain',
      );
    };

    const addThroughWallet = async () => {
      if (!ethereum) return;
      await withTimeout(
        ethereum.request({
          method: 'wallet_addEthereumChain',
          params: [addParams],
        }),
        12_000,
        'wallet_addEthereumChain',
      );
    };

    const currentHex = ethereum
      ? await withTimeout(ethereum.request({ method: 'eth_chainId' }).catch(() => null), 8_000, 'eth_chainId')
      : null;
    if (typeof currentHex === 'string') {
      const parsed = Number.parseInt(currentHex, 16);
      setProviderChainId(Number.isFinite(parsed) ? parsed : null);
      if (currentHex === hexChainId) {
        return;
      }
    }

    // Try Wagmi first (works when wallet chain metadata is already healthy).
    try {
      await withTimeout(switchChainAsync({ chainId: TARGET_CHAIN_ID }), 12_000, 'switchChainAsync');
    } catch {
      // Fall back to direct wallet RPC path below.
    }

    if (!ethereum) {
      if (activeChainId !== TARGET_CHAIN_ID) {
        throw new Error(
          `Wallet provider unavailable for switching. Switch manually to chain ${TARGET_CHAIN_ID} and retry.`,
        );
      }
      return;
    }

    const afterWagmiHex = await withTimeout(
      ethereum.request({ method: 'eth_chainId' }).catch(() => null),
      8_000,
      'eth_chainId',
    );
    if (afterWagmiHex !== hexChainId) {
      try {
        await switchThroughWallet();
      } catch (switchError) {
        const switchCode = readErrorCode(switchError);
        const switchMessage = readErrorMessage(switchError);
        const looksUnknownChain =
          switchCode === 4902 || switchMessage.toLowerCase().includes('unrecognized chain');
        if (!looksUnknownChain) {
          throw new Error(`Wallet refused chain switch: ${switchMessage}`);
        }
        try {
          await addThroughWallet();
        } catch (addError) {
          const addMessage = readErrorMessage(addError);
          const addMessageLower = addMessage.toLowerCase();
          if (isHttpRpc && addMessageLower.includes('https') && addMessageLower.includes('rpcurl')) {
            throw new Error(
              `Keplr requires an HTTPS RPC URL to add chain ${TARGET_CHAIN_ID}, but current RPC is ${TARGET_RPC_URL}. ` +
                `Use MetaMask for local HTTP RPC, or expose this RPC via HTTPS and set VITE_RPC_URL to that endpoint.`,
            );
          }
          throw new Error(`Wallet could not add chain ${TARGET_CHAIN_ID}: ${addMessage}`);
        }
        await switchThroughWallet();
      }
    }

    const finalChainHex = await withTimeout(
      ethereum.request({ method: 'eth_chainId' }).catch(() => null),
      8_000,
      'eth_chainId',
    );
    if (typeof finalChainHex === 'string') {
      const parsed = Number.parseInt(finalChainHex, 16);
      setProviderChainId(Number.isFinite(parsed) ? parsed : null);
    }
    if (finalChainHex !== hexChainId) {
      throw new Error(
        `Unable to switch wallet to chain ${TARGET_CHAIN_ID}. Open MetaMask, select chain ${TARGET_CHAIN_ID}, then retry.`,
      );
    }

    // Validate wallet RPC endpoint by executing a chain RPC.
    try {
      await withTimeout(ethereum.request({ method: 'eth_blockNumber' }), 8_000, 'eth_blockNumber');
    } catch {
      await addThroughWallet();
      await switchThroughWallet();
      await withTimeout(ethereum.request({ method: 'eth_blockNumber' }), 8_000, 'eth_blockNumber');
    }
  }, [activeChainId, switchChainAsync]);

  const connectWallet = useCallback(async (requestedConnectorId?: string) => {
    if (isWalletConnected) return;
    if (connectors.length === 0) {
      setLaunchNotice('error', 'No wallet connector found. Install a wallet extension or configure WalletConnect.');
      return;
    }

    const hasKeplrProvider =
      typeof window !== 'undefined' &&
      Boolean(
        (window as typeof window & { keplr?: unknown; ethereum?: { isKeplr?: boolean } }).keplr ||
          (window as typeof window & { ethereum?: { isKeplr?: boolean } }).ethereum?.isKeplr,
      );
    const orderedConnectors = [...connectors].sort((left, right) => {
      const rank = (connector: (typeof connectors)[number]) => {
        const idText = `${connector.id} ${connector.name}`.toLowerCase();
        let score = 0;
        if (connector.type === 'injected') score -= 20;
        if (hasKeplrProvider && idText.includes('keplr')) score -= 10;
        if (!hasKeplrProvider && idText.includes('metamask')) score -= 8;
        if (idText.includes('walletconnect')) score += 5;
        return score;
      };
      return rank(left) - rank(right);
    });
    const candidates = requestedConnectorId
      ? orderedConnectors.filter((connector) => connector.id === requestedConnectorId)
      : orderedConnectors;

    let lastError: Error | null = null;
    for (const connector of candidates) {
      try {
        await connectAsync({ connector });
        if (STRICT_WALLET_RPC_MATCH) {
          await forceWalletToTargetChain();
        }
        setLaunchNotice('success', `Wallet connected via ${connector.name}.`);
        return;
      } catch (error) {
        lastError = error as Error;
      }
    }

    setLaunchNotice(
      'error',
      `Wallet connect failed: ${lastError?.message ?? 'No compatible wallet detected in this browser context.'}`,
    );
  }, [connectAsync, connectors, forceWalletToTargetChain, isWalletConnected, setLaunchNotice]);

  const ensureTargetChain = useCallback(async (opts?: { attemptSwitch?: boolean }): Promise<boolean> => {
    const attemptSwitch = opts?.attemptSwitch ?? false;
    if (!isWalletConnected) {
      setLaunchNotice('error', 'Connect your wallet first.');
      return false;
    }
    try {
      selectedChainIdStore.set(TARGET_CHAIN_ID);

      // Default flow: never trigger wallet network update/switch RPCs on submit.
      if (!STRICT_WALLET_RPC_MATCH && !attemptSwitch) {
        let resolvedChainId = activeChainId;
        if (resolvedChainId === undefined) {
          const ethereum = browserEthereum();
          if (ethereum) {
            const chainHex = await withTimeout(
              ethereum.request({ method: 'eth_chainId' }).catch(() => null),
              6_000,
              'eth_chainId',
            );
            if (typeof chainHex === 'string') {
              const parsed = Number.parseInt(chainHex, 16);
              if (Number.isFinite(parsed)) {
                setProviderChainId(parsed);
                resolvedChainId = parsed;
              }
            }
          }
        }
        if (resolvedChainId !== TARGET_CHAIN_ID) {
          setLaunchNotice(
            'error',
            `Wallet is on chain ${resolvedChainId ?? 'unknown'}. Switch to ${TARGET_CHAIN_ID} in wallet first, then retry.`,
          );
          return false;
        }
        return true;
      }

      const alreadyOnTargetChain = activeChainId === TARGET_CHAIN_ID;
      if (!(alreadyOnTargetChain && !STRICT_WALLET_RPC_MATCH)) {
        await forceWalletToTargetChain();
      }
      const contractAddress = import.meta.env.VITE_TANGLE_CONTRACT?.trim();
      if (contractAddress && isAddress(contractAddress)) {
        const targetCode = await jsonRpcCall<string>(TARGET_RPC_URL, 'eth_getCode', [contractAddress, 'latest']);
        if (targetCode === '0x') {
          setLaunchNotice(
            'error',
            `RPC ${TARGET_RPC_URL} is reachable but has no contract code at ${contractAddress}. Check deploy-local RPC host/port.`,
          );
          return false;
        }

        const ethereum = browserEthereum();
        if (ethereum) {
          const readWalletCode = async (): Promise<string | null> => {
            const raw = await ethereum
              .request({
                method: 'eth_getCode',
                params: [contractAddress, 'latest'],
              })
              .catch(() => null);
            return typeof raw === 'string' ? raw : null;
          };

          let walletCode = await readWalletCode();
          if (!walletCode || walletCode === '0x' || walletCode.toLowerCase() !== targetCode.toLowerCase()) {
            const mismatchText =
              `Wallet RPC does not match app RPC for chain ${TARGET_CHAIN_ID}. ` +
              `Set wallet RPC URL to ${TARGET_RPC_URL}, reconnect, and retry.`;
            if (!STRICT_WALLET_RPC_MATCH) {
              setLaunchNotice('info', `${mismatchText} Continuing with current wallet RPC for this session.`);
              return true;
            }

            // Self-heal stale chain RPC metadata (common when HTTPS tunnel rotates between runs).
            const addParams = {
              chainId: `0x${TARGET_CHAIN_ID.toString(16)}`,
              chainName: TARGET_CHAIN_NAME,
              nativeCurrency: {
                name: TARGET_CURRENCY_SYMBOL,
                symbol: TARGET_CURRENCY_SYMBOL,
                decimals: 18,
              },
              rpcUrls: [TARGET_RPC_URL],
              ...(TARGET_EXPLORER_URL ? { blockExplorerUrls: [TARGET_EXPLORER_URL] } : {}),
            };
            try {
              await withTimeout(
                ethereum.request({
                  method: 'wallet_addEthereumChain',
                  params: [addParams],
                }),
                12_000,
                'wallet_addEthereumChain',
              );
              await withTimeout(
                ethereum.request({
                  method: 'wallet_switchEthereumChain',
                  params: [{ chainId: addParams.chainId }],
                }),
                12_000,
                'wallet_switchEthereumChain',
              );
              walletCode = await readWalletCode();
            } catch (healError) {
              setLaunchNotice(
                'error',
                `Wallet RPC does not match app RPC and auto-fix failed: ${readErrorMessage(healError)}. ` +
                  `Set wallet RPC URL to ${TARGET_RPC_URL}, reconnect, and retry.`,
              );
              return false;
            }

            if (!walletCode || walletCode === '0x' || walletCode.toLowerCase() !== targetCode.toLowerCase()) {
              setLaunchNotice('error', mismatchText);
              return false;
            }
          }
        }
      }
      setLaunchFeedback((current) => (current?.tone === 'error' ? null : current));
      return true;
    } catch (error) {
      setLaunchNotice(
        'error',
        `Wallet network sync failed for chain ${TARGET_CHAIN_ID} (${TARGET_RPC_URL}): ${(error as Error).message}`,
      );
      return false;
    }
  }, [activeChainId, forceWalletToTargetChain, isWalletConnected, setLaunchNotice]);

  const copyWalletAddress = useCallback(async () => {
    if (!connectedWallet) return;
    const copied = await copyText(connectedWallet);
    if (!copied) return;
    setWalletCopied(true);
    setTimeout(() => setWalletCopied(false), 1400);
  }, [connectedWallet]);

  useEffect(() => {
    selectedChainIdStore.set(TARGET_CHAIN_ID);
  }, []);

  useEffect(() => {
    const saved = loadSavedToken();
    const initial = saved || DEMO_TOKEN;
    setToken(initial);
    setTokenInput(initial);

    const savedTheme = localStorage.getItem('openclaw_ui_theme');
    if (!savedTheme) {
      document.documentElement.setAttribute('data-theme', 'dark');
      localStorage.setItem('openclaw_ui_theme', 'dark');
    }

  }, []);

  useEffect(() => {
    if (hydratedFromUrlRef.current) return;
    if (typeof window === 'undefined') return;
    hydratedFromUrlRef.current = true;

    const params = new URLSearchParams(window.location.search);
    const maybeView = params.get('view');
    const maybePanel = params.get('panel');
    const maybeStep = params.get('step');
    const maybeInstance = params.get('instance');
    const maybeSession = params.get('session');

    if (isSurfaceTab(maybeView)) {
      setSurfaceTab(maybeView);
    }
    if (isMainTab(maybePanel)) {
      setTab(maybePanel);
    }
    if (isWizardStep(maybeStep)) {
      setWizardStep(Number(maybeStep) as WizardStep);
      if ((maybeView === 'launch' || !maybeView) && maybeStep !== '1') {
        setWizardOpen(true);
      }
    }
    if (maybeInstance) {
      setSelectedId(maybeInstance);
    }
    if (maybeSession) {
      setActiveSessionId(maybeSession);
    }
  }, []);

  useEffect(() => {
    if (!isWalletConnected) {
      setWalletMenuOpen(false);
    }
  }, [isWalletConnected]);

  useEffect(() => {
    if (!walletMenuOpen) return;

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (walletMenuRef.current?.contains(target)) return;
      if (walletMenuButtonRef.current?.contains(target)) return;
      setWalletMenuOpen(false);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setWalletMenuOpen(false);
      walletMenuButtonRef.current?.focus();
    };

    window.addEventListener('pointerdown', onPointerDown);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [walletMenuOpen]);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const refreshViaWallet = async () => {
      if (!isWalletConnected || !connectedWallet) {
        if (!cancelled) {
          setWalletBalanceRpcHex(null);
          setWalletBalanceTargetRpcHex(null);
          setIsWalletBalanceRpcLoading(false);
        }
        return;
      }
      const ethereum = browserEthereum();
      if (!ethereum) {
        if (!cancelled) {
          setWalletBalanceRpcHex(null);
          setWalletBalanceTargetRpcHex(null);
          setIsWalletBalanceRpcLoading(false);
        }
      }
      if (!cancelled) {
        setIsWalletBalanceRpcLoading(true);
      }
      try {
        let walletRpcHex: string | null = null;
        if (ethereum) {
          try {
            const value = await ethereum.request({
              method: 'eth_getBalance',
              params: [connectedWallet, 'latest'],
            });
            if (typeof value === 'string') {
              walletRpcHex = value;
            }
          } catch {
            walletRpcHex = null;
          }
        }

        const targetRpcHex = await jsonRpcCall<string>(TARGET_RPC_URL, 'eth_getBalance', [connectedWallet, 'latest']);
        if (!cancelled) {
          setWalletBalanceRpcHex(walletRpcHex);
          setWalletBalanceTargetRpcHex(typeof targetRpcHex === 'string' ? targetRpcHex : null);
        }
      } finally {
        if (!cancelled) {
          setIsWalletBalanceRpcLoading(false);
        }
      }
    };

    if (!walletBalance && isWalletConnected && connectedWallet) {
      void refreshViaWallet();
      timer = setInterval(() => {
        void refreshViaWallet();
      }, 15_000);
    } else if (walletBalance && !cancelled) {
      setWalletBalanceRpcHex(null);
      setWalletBalanceTargetRpcHex(null);
      setIsWalletBalanceRpcLoading(false);
    }

    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [activeChainId, connectedWallet, isWalletConnected, walletBalance]);

  const selectedInstance = useMemo(
    () => instances.find((instance) => instance.id === selectedId) ?? null,
    [instances, selectedId],
  );
  const selectedTemplate = useMemo(
    () => templates.find((pack) => pack.id === provisionTemplateId) ?? null,
    [provisionTemplateId, templates],
  );

  const selectedApiBase = selectedInstance
    ? `/instances/${encodeURIComponent(selectedInstance.id)}`
    : '';
  const selectedAuthMode: SessionSource =
    selectedInstance?.uiAccess.authMode === 'access_token' ? 'access_token' : 'wallet_signature';
  const selectedScopedSession = selectedId ? scopedSessions[selectedId] ?? null : null;
  const scopedSessionIsValid = Boolean(
    selectedScopedSession && selectedScopedSession.expiresAt > Math.floor(Date.now() / 1000) + 30,
  );
  const scopedToken = scopedSessionIsValid ? selectedScopedSession?.token ?? '' : '';

  const sessions = useSessions(selectedApiBase, scopedToken || null);
  const createSessionMutation = useCreateSession(selectedApiBase, scopedToken || null);
  const deleteSessionMutation = useDeleteSession(selectedApiBase, scopedToken || null);
  const renameSessionMutation = useRenameSession(selectedApiBase, scopedToken || null);
  const [activeSessionId, setActiveSessionId] = useState('');

  const sessionStream = useSessionStream({
    apiUrl: selectedApiBase,
    token: scopedToken || null,
    sessionId: activeSessionId,
    enabled: Boolean(selectedApiBase && scopedToken && activeSessionId),
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
    return () => {
      if (sessionDeleteTimerRef.current) {
        clearTimeout(sessionDeleteTimerRef.current);
      }
    };
  }, []);

  const undoPendingSessionDelete = useCallback(() => {
    if (sessionDeleteTimerRef.current) {
      clearTimeout(sessionDeleteTimerRef.current);
      sessionDeleteTimerRef.current = null;
    }
    if (pendingSessionDelete) {
      setNotice({ tone: 'info', text: `Deletion canceled for "${pendingSessionDelete.title}".` });
    }
    setPendingSessionDelete(null);
  }, [pendingSessionDelete]);

  const queueSessionDelete = useCallback(
    (sessionId: string, sessionTitle: string) => {
      if (sessionDeleteTimerRef.current) {
        clearTimeout(sessionDeleteTimerRef.current);
        sessionDeleteTimerRef.current = null;
      }

      setPendingSessionDelete({ id: sessionId, title: sessionTitle });
      setNotice({
        tone: 'info',
        text: `Deleting "${sessionTitle}" in 8 seconds. Undo if this was accidental.`,
      });

      sessionDeleteTimerRef.current = setTimeout(() => {
        deleteSessionMutation.mutate(sessionId, {
          onSuccess: () => {
            sessionDeleteTimerRef.current = null;
            if (activeSessionId === sessionId) {
              setActiveSessionId('');
            }
            setPendingSessionDelete(null);
            setNotice({ tone: 'success', text: `Session "${sessionTitle}" deleted.` });
          },
          onError: (error) => {
            sessionDeleteTimerRef.current = null;
            setPendingSessionDelete(null);
            setNotice({ tone: 'error', text: `Delete failed: ${(error as Error).message}` });
          },
        });
      }, 8_000);
    },
    [activeSessionId, deleteSessionMutation],
  );

  useEffect(() => {
    if (!pendingSessionDelete) return;
    undoPendingSessionDelete();
    // Only cancel on instance/API scope switch, not when pending flag is first set.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedApiBase]);

  useEffect(() => {
    setInstanceAccess(null);
    setTab('workspace');
    setTerminalOutput('');
    setTeeOutput('');
    setOwnerSessionFeedback(null);
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
        const owned = connectedWallet
          ? discoveredInstances.find((item) => sameAddress(item.owner, connectedWallet))
          : null;
        if (current) {
          const active = discoveredInstances.find((item) => item.id === current);
          if (active) {
            if (!connectedWallet || sameAddress(active.owner, connectedWallet)) {
              return current;
            }
            if (owned) return owned.id;
            return current;
          }
        }
        if (owned) return owned.id;
        return discoveredInstances[0]?.id ?? '';
      });
    } catch (error) {
      const message = (error as Error).message || 'refresh failed';
      const lower = message.toLowerCase();
      const unauthorized = lower.includes('unauthorized') || lower.includes('invalid or expired bearer');
      if (unauthorized && token !== DEMO_TOKEN) {
        saveToken(DEMO_TOKEN);
        setToken(DEMO_TOKEN);
        setTokenInput(DEMO_TOKEN);
        setNotice({
          tone: 'info',
          text: 'Session token was invalid. Switched to local operator token and retried refresh.',
        });
        try {
          const [templatePacks, discoveredInstances] = await Promise.all([
            fetchTemplates(DEMO_TOKEN),
            fetchInstances(DEMO_TOKEN),
          ]);
          setTemplates(templatePacks);
          setInstances(discoveredInstances);
          setSelectedId((current) => {
            const owned = connectedWallet
              ? discoveredInstances.find((item) => sameAddress(item.owner, connectedWallet))
              : null;
            if (current) {
              const active = discoveredInstances.find((item) => item.id === current);
              if (active) {
                if (!connectedWallet || sameAddress(active.owner, connectedWallet)) {
                  return current;
                }
                if (owned) return owned.id;
                return current;
              }
            }
            if (owned) return owned.id;
            return discoveredInstances[0]?.id ?? '';
          });
        } catch (retryError) {
          setNotice({
            tone: 'error',
            text: `Refresh retry failed: ${(retryError as Error).message}`,
          });
        }
      } else {
        setNotice({ tone: 'error', text: `Refresh failed: ${message}` });
      }
    } finally {
      setIsRefreshing(false);
    }
  }, [connectedWallet, token]);

  useEffect(() => {
    if (!token.trim()) return;
    void refresh();
  }, [token, refresh]);

  useEffect(() => {
    if (templates.length === 0) return;
    if (!templates.some((pack) => pack.id === provisionTemplateId)) {
      setProvisionTemplateId(templates[0].id);
    }
  }, [provisionTemplateId, templates]);

  const onSaveToken = useCallback(() => {
    const normalized = tokenInput.trim();
    saveToken(normalized);
    setToken(normalized);
    setNotice({ tone: 'success', text: normalized ? 'Bearer token saved.' : 'Bearer token cleared.' });
  }, [tokenInput]);

  const onUseDemoToken = useCallback(() => {
    saveToken(DEMO_TOKEN);
    setToken(DEMO_TOKEN);
    setTokenInput(DEMO_TOKEN);
    setNotice({ tone: 'info', text: 'Local token applied.' });
  }, []);

  const signWithConnectedWallet = useCallback(
    async (walletAddress: string, message: string): Promise<string> => {
      let wagmiError: Error | null = null;
      try {
        const signature = await signMessageAsync({
          account: walletAddress as `0x${string}`,
          message,
        });
        if (typeof signature === 'string' && signature.trim().length > 0) {
          return signature;
        }
        wagmiError = new Error('Wallet returned an empty signature.');
      } catch (error) {
        wagmiError = error as Error;
      }

      try {
        return await signWalletMessage(walletAddress, message);
      } catch (fallbackError) {
        const hasProvider = Boolean(browserEthereum());
        const providerHint = hasProvider ? '' : ' No injected wallet provider is available on this page.';
        const secureContextHint =
          typeof window !== 'undefined' && !window.isSecureContext
            ? ' This page is not in a secure context; wallet signing may require https/localhost.'
            : '';
        throw new Error(
          `Wallet signature failed: ${
            wagmiError?.message ?? (fallbackError as Error).message
          }.${providerHint}${secureContextHint}`,
        );
      }
    },
    [signMessageAsync],
  );

  const ensureScopedSession = useCallback(
    async (instance: InstanceView): Promise<string> => {
      const existing = scopedSessions[instance.id];
      const now = Math.floor(Date.now() / 1000);
      if (existing && existing.expiresAt > now + 30) {
        return existing.token;
      }

      setIsCreatingScopedSession(true);
      try {
        const authMode: SessionSource =
          instance.uiAccess.authMode === 'access_token' ? 'access_token' : 'wallet_signature';
        const session =
          authMode === 'access_token'
            ? await (async () => {
                const accessToken = (instanceAccessTokenInput[instance.id] ?? '').trim();
                if (!accessToken) {
                  throw new Error('Enter the instance access token first.');
                }
                return createSessionFromAccessToken(token.trim(), {
                  instanceId: instance.id,
                  accessToken,
                });
              })()
            : await (async () => {
                if (!connectedWallet) {
                  throw new Error('Connect the owner wallet to create a scoped session.');
                }
                if (!sameAddress(instance.owner, connectedWallet)) {
                  throw new Error(
                    `Selected instance is owned by ${truncateAddress(instance.owner)}; connect the owner wallet.`,
                  );
                }
                try {
                  const challenge = await requestWalletChallenge(token.trim(), {
                    instanceId: instance.id,
                    walletAddress: connectedWallet,
                  });
                  const signature = await signWithConnectedWallet(connectedWallet, challenge.message);
                  return verifyWalletSession(token.trim(), {
                    challengeId: challenge.challengeId,
                    signature,
                  });
                } catch (signatureError) {
                  if (DEV_INSTANCE_ACCESS_TOKEN) {
                    return createSessionFromAccessToken(token.trim(), {
                      instanceId: instance.id,
                      accessToken: DEV_INSTANCE_ACCESS_TOKEN,
                    });
                  }
                  throw signatureError;
                }
              })();

        setScopedSessions((current) => ({
          ...current,
          [instance.id]: {
            token: session.token,
            expiresAt: session.expiresAt,
            owner: session.owner,
            instanceId: session.instanceId,
            source: authMode,
          },
        }));
        return session.token;
      } finally {
        setIsCreatingScopedSession(false);
      }
    },
    [connectedWallet, instanceAccessTokenInput, scopedSessions, signWithConnectedWallet, token],
  );

  const onCreateScopedSession = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      setOwnerSessionFeedback(null);
      await ensureScopedSession(selectedInstance);
      const text =
        selectedAuthMode === 'wallet_signature'
          ? 'Owner wallet session created.'
          : 'Access-token session created.';
      setOwnerSessionFeedback({ tone: 'success', text });
      setWorkspaceFeedback(null);
      setNotice({
        tone: 'success',
        text,
      });
    } catch (error) {
      const text = `Session creation failed: ${(error as Error).message}`;
      setOwnerSessionFeedback({ tone: 'error', text });
      setNotice({ tone: 'error', text });
    }
  }, [ensureScopedSession, selectedAuthMode, selectedInstance]);

  const startProvisionFlow = useCallback((variant: ClawVariant) => {
    const generated = generateProvisionIdentity(variant);
    setProvisionVariant(variant);
    setProvisionName(generated.name);
    setProvisionSubdomain(generated.subdomain);
    setSurfaceTab('launch');
    setWizardOpen(true);
    setWizardStep(1);
    setLaunchFeedback(null);
  }, []);

  const regenerateProvisionIdentity = useCallback(() => {
    const generated = generateProvisionIdentity(provisionVariant);
    setProvisionName(generated.name);
    setProvisionSubdomain(generated.subdomain);
  }, [provisionVariant]);

  const resolveServiceId = useCallback(
    (target: 'standard' | 'tee'): bigint | null =>
      parseServiceId(target === 'tee' ? teeServiceIdInput : standardServiceIdInput),
    [standardServiceIdInput, teeServiceIdInput],
  );

  const onProvisionInstance = useCallback(async () => {
    if (!provisionName.trim()) {
      setLaunchNotice('error', 'Instance name is required.');
      return;
    }
    if (!provisionTemplateId.trim()) {
      setLaunchNotice('error', 'Template pack is required.');
      return;
    }

    setIsProvisioning(true);
    try {
      if (DEMO_MODE) {
        if (!token.trim()) {
          setLaunchNotice('error', 'Save a bearer token before provisioning.');
          return;
        }
        const created = await provisionInstance(token, {
          name: provisionName.trim(),
          clawVariant: provisionVariant,
          templatePackId: provisionTemplateId,
          executionTarget: provisionExecutionTarget,
        });
        setInstances((current) => [created, ...current.filter((item) => item.id !== created.id)]);
        setSelectedId(created.id);
        setWizardOpen(false);
        setSurfaceTab('workspace');
        setLaunchNotice('success', `Provisioned ${created.name}.`);
        return;
      }

      if (!isWalletConnected) {
        setLaunchNotice('error', 'Connect your wallet before submitting on-chain jobs.');
        return;
      }
      if (!(await ensureTargetChain())) {
        return;
      }
      const serviceId = resolveServiceId(provisionExecutionTarget);
      if (serviceId === null) {
        setLaunchNotice(
          'error',
          `Missing ${provisionExecutionTarget} service ID. Set it in UI advanced settings or .env.`,
        );
        return;
      }

      const configJson = JSON.stringify({
        claw_variant: provisionVariant,
        ui: {
          expose_public_url: true,
          subdomain: provisionSubdomain.trim() || provisionName.trim(),
          auth_mode: 'wallet_signature',
        },
      });

      const args = encodeAbiParameters(
        [
          { name: 'name', type: 'string' },
          { name: 'template_pack_id', type: 'string' },
          { name: 'config_json', type: 'string' },
        ],
        [provisionName.trim(), provisionTemplateId.trim(), configJson],
      );

      const hash = await submitJob({
        serviceId,
        jobId: JOB_CREATE,
        args,
        label: `Create ${provisionName.trim()}`,
      });
      if (!hash) {
        await new Promise((resolve) => setTimeout(resolve, 0));
        const latestError = txErrorRef.current;
        const latestStatus = txStatusRef.current;
        const normalizedError = (latestError ?? '').toLowerCase();
        if (normalizedError.includes('requested resource not available')) {
          setLaunchNotice(
            'error',
            'Create transaction was not submitted: wallet has another pending request. Open wallet extension, resolve pending network/sign prompts, then retry.',
          );
          return;
        }
        const fallback =
          latestStatus === 'failed'
            ? 'wallet rejected the transaction or provider returned an error'
            : 'wallet did not submit the transaction';
        setLaunchNotice('error', `Create transaction was not submitted: ${latestError ?? fallback}.`);
        return;
      }
      setLaunchNotice('info', `Create job submitted (${hash}). Waiting for operator execution.`);
      setWizardOpen(false);
      setSurfaceTab('instances');
      setTimeout(() => void refresh(), 2500);
    } catch (error) {
      setLaunchNotice('error', `Provision failed: ${(error as Error).message}`);
    } finally {
      setIsProvisioning(false);
    }
  }, [
    isWalletConnected,
    provisionExecutionTarget,
    provisionName,
    provisionTemplateId,
    provisionVariant,
    provisionSubdomain,
    refresh,
    resolveServiceId,
    submitJob,
    token,
    txError,
    ensureTargetChain,
    setLaunchNotice,
  ]);

  const onSubmitLifecycleJob = useCallback(
    async (jobId: number, label: string) => {
      if (!selectedInstance) return;
      if (!connectedWallet || !sameAddress(selectedInstance.owner, connectedWallet)) {
        setNotice({
          tone: 'error',
          text: `Action blocked: selected instance is owned by ${truncateAddress(selectedInstance.owner)}. Select your own instance.`,
        });
        return;
      }
      if (!isWalletConnected) {
        setNotice({ tone: 'error', text: 'Connect your wallet before submitting lifecycle jobs.' });
        return;
      }
      if (!(await ensureTargetChain())) {
        return;
      }
      const target = selectedInstance.executionTarget === 'tee' ? 'tee' : 'standard';
      const serviceId = resolveServiceId(target);
      if (serviceId === null) {
        setNotice({
          tone: 'error',
          text: `Missing ${target} service ID. Set it in advanced service routing.`,
        });
        return;
      }

      const args = encodeAbiParameters([{ name: 'instance_id', type: 'string' }], [selectedInstance.id]);
      const hash = await submitJob({
        serviceId,
        jobId,
        args,
        label,
      });
      if (!hash) {
        setNotice({
          tone: 'error',
          text: `${label} transaction was not submitted${txError ? `: ${txError}` : '.'}`,
        });
        return;
      }
      setNotice({ tone: 'info', text: `${label} submitted (${hash}). Refreshing status...` });
      setTimeout(() => void refresh(), 2500);
    },
    [connectedWallet, isWalletConnected, refresh, resolveServiceId, selectedInstance, submitJob, txError, ensureTargetChain],
  );

  const confirmAndDeleteInstance = useCallback(async () => {
    if (!selectedInstance) return;
    const typed = window.prompt(`Type "${selectedInstance.name}" to confirm delete.`);
    if (typed === null) return;
    if (typed.trim() !== selectedInstance.name) {
      setNotice({ tone: 'error', text: 'Delete canceled: name did not match exactly.' });
      return;
    }
    await onSubmitLifecycleJob(JOB_DELETE, 'Delete Instance');
  }, [onSubmitLifecycleJob, selectedInstance]);

  const onOneClickSetup = useCallback(async () => {
    if (!selectedInstance) return;
    if (!selectedInstance.runtime.setupCommand && !selectedInstance.runtime.setupUrl) {
      setWorkspaceNotice('error', 'Setup is not available for this runtime backend yet.');
      return;
    }
    setIsSetupStarting(true);
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const updated = await startSetup(scoped, selectedInstance.id, {});
      setInstances((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setWorkspaceNotice('success', `Setup started for ${selectedInstance.name}.`);
    } catch (error) {
      setWorkspaceNotice('error', `Setup failed: ${(error as Error).message}`);
    } finally {
      setIsSetupStarting(false);
    }
  }, [ensureScopedSession, selectedInstance, setWorkspaceNotice]);

  const onSetupWithEnv = useCallback(async () => {
    if (!selectedInstance) return;
    if (!selectedInstance.runtime.setupCommand && !selectedInstance.runtime.setupUrl) {
      setWorkspaceNotice('error', 'Setup is not available for this runtime backend yet.');
      return;
    }
    setIsSetupStarting(true);
    try {
      const env = parseEnvText(setupEnvText);
      const scoped = await ensureScopedSession(selectedInstance);
      const updated = await startSetup(scoped, selectedInstance.id, env);
      setInstances((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setWorkspaceNotice('success', `Setup started with ${Object.keys(env).length} env override(s).`);
    } catch (error) {
      setWorkspaceNotice('error', `Advanced setup failed: ${(error as Error).message}`);
    } finally {
      setIsSetupStarting(false);
    }
  }, [ensureScopedSession, selectedInstance, setWorkspaceNotice, setupEnvText]);

  const onFetchInstanceAccess = useCallback(async () => {
    if (!selectedInstance) return;
    if (!selectedInstance.runtime.hasUiBearerToken) {
      setNotice({
        tone: 'error',
        text: 'Instance UI bearer token is not configured for this runtime yet.',
      });
      return;
    }
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const access = await getInstanceAccess(scoped, selectedInstance.id);
      setInstanceAccess(access);
      setNotice({ tone: 'success', text: 'Instance access credentials retrieved.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `Access retrieval failed: ${(error as Error).message}` });
    }
  }, [ensureScopedSession, selectedInstance]);

  const onRunTerminalCommand = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const output = await runTerminalCommand(scoped, selectedInstance.id, terminalCommand);
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
  }, [ensureScopedSession, selectedInstance, terminalCommand]);

  const onQuickChat = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const session = await createChatSession(scoped, selectedInstance.id, 'Quick prompt');
      await sendChatMessage(scoped, selectedInstance.id, session.id, quickChatPrompt);
      const messages = await getSessionMessages(scoped, selectedInstance.id, session.id);
      setNotice({ tone: 'info', text: `Assistant reply: ${firstAssistantReply(messages)}` });
    } catch (error) {
      setNotice({ tone: 'error', text: `Quick chat failed: ${(error as Error).message}` });
    }
  }, [ensureScopedSession, quickChatPrompt, selectedInstance]);

  const onSshUpsert = useCallback(async (method: 'POST' | 'DELETE') => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      await updateSshKey(
        scoped,
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
  }, [ensureScopedSession, selectedInstance, sshPublicKey, sshUsername]);

  const onTeePublicKey = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const payload = await teePublicKey(scoped, selectedInstance.id);
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE public key fetched.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE public key failed: ${(error as Error).message}` });
    }
  }, [ensureScopedSession, selectedInstance]);

  const onTeeAttestation = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const payload = await teeAttestation(scoped, selectedInstance.id);
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE attestation fetched.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE attestation failed: ${(error as Error).message}` });
    }
  }, [ensureScopedSession, selectedInstance]);

  const onTeeSealedSecret = useCallback(async () => {
    if (!selectedInstance) return;
    try {
      const scoped = await ensureScopedSession(selectedInstance);
      const payload = await teeSealedSecrets(scoped, selectedInstance.id, {
        algorithm: teeAlgorithm,
        ciphertext: parseByteSequence(teeCiphertext),
        nonce: parseByteSequence(teeNonce),
      });
      setTeeOutput(JSON.stringify(payload, null, 2));
      setNotice({ tone: 'success', text: 'TEE sealed secret payload submitted.' });
    } catch (error) {
      setNotice({ tone: 'error', text: `TEE sealed secret failed: ${(error as Error).message}` });
    }
  }, [ensureScopedSession, selectedInstance, teeAlgorithm, teeCiphertext, teeNonce]);

  const hasToken = token.trim().length > 0;
  const hasScopedSession = scopedToken.trim().length > 0;
  const canProvision = Boolean(provisionName.trim() && provisionTemplateId.trim());
  const selectedProvisionServiceId = resolveServiceId(provisionExecutionTarget);
  const totalInstances = instances.length;
  const runningInstances = useMemo(
    () => instances.filter((instance) => instance.status === 'running').length,
    [instances],
  );
  const teeInstances = useMemo(
    () => instances.filter((instance) => instance.executionTarget === 'tee').length,
    [instances],
  );
  const selectedInstanceServiceId = selectedInstance
    ? resolveServiceId(selectedInstance.executionTarget === 'tee' ? 'tee' : 'standard')
    : null;
  const selectedInstanceSetupCapable = Boolean(
    selectedInstance && (selectedInstance.runtime.setupCommand || selectedInstance.runtime.setupUrl),
  );
  const selectedInstanceAccessCapable = Boolean(selectedInstance?.runtime.hasUiBearerToken);
  const selectedInstanceOwnedByWallet = Boolean(
    selectedInstance && connectedWallet && sameAddress(selectedInstance.owner, connectedWallet),
  );
  const pendingTxAgeMs = pendingTxSinceMs !== null ? Date.now() - pendingTxSinceMs : 0;
  const stalePendingTx = txStatus === 'pending' && pendingTxAgeMs > 90_000;
  const switchingAgeMs = switchRequestSinceMs !== null ? Date.now() - switchRequestSinceMs : 0;
  const staleSwitchRequest = isSwitchingChain && switchingAgeMs > 20_000;
  const chainSwitchBusy = isSwitchingChain && !staleSwitchRequest;
  const lifecycleTxBusy = txStatus === 'signing' || (txStatus === 'pending' && !stalePendingTx) || chainSwitchBusy;
  const walletLabel = isWalletConnected ? truncateAddress(connectedWallet) : 'Wallet Disconnected';
  const walletProviderBalanceWei = parseRpcHexToBigint(walletBalanceRpcHex);
  const targetRpcBalanceWei = parseRpcHexToBigint(walletBalanceTargetRpcHex);
  const walletProviderBalanceLabel =
    walletProviderBalanceWei !== null
      ? `${Number.parseFloat(formatUnits(walletProviderBalanceWei, 18)).toFixed(4)} ${TARGET_CURRENCY_SYMBOL}`
      : null;
  const targetRpcBalanceLabel =
    targetRpcBalanceWei !== null
      ? `${Number.parseFloat(formatUnits(targetRpcBalanceWei, 18)).toFixed(4)} ${TARGET_CURRENCY_SYMBOL}`
      : null;
  const walletBalanceLabel =
    isWalletConnected && walletBalance
      ? `${Number.parseFloat(formatUnits(walletBalance.value, walletBalance.decimals)).toFixed(4)} ${walletBalance.symbol}`
      : targetRpcBalanceLabel
        ? targetRpcBalanceLabel
        : walletProviderBalanceLabel
          ? walletProviderBalanceLabel
        : isWalletBalanceLoading || isWalletBalanceRpcLoading
        ? 'Loading…'
        : 'n/a';
  const chainLabel =
    activeChainId === undefined
      ? 'n/a'
      : activeChainId === TARGET_CHAIN_ID
        ? `${TARGET_CHAIN_NAME} (${activeChainId})`
      : `Chain ${activeChainId}`;
  const createSubmitDisabledReason = useMemo(() => {
    if (isProvisioning) return 'Provision request already in progress.';
    if (!canProvision) return 'Instance name and template are required.';
    if (DEMO_MODE) return null;
    if (!isWalletConnected) return 'Connect wallet before submitting.';
    if (selectedProvisionServiceId === null) return `Missing ${provisionExecutionTarget} service ID.`;
    if (chainSwitchBusy) return 'Waiting for wallet chain switch confirmation.';
    if (txStatus === 'signing') return 'Waiting for wallet signature.';
    if (txStatus === 'pending' && !stalePendingTx) return 'Previous transaction is still pending.';
    return null;
  }, [
    isProvisioning,
    canProvision,
    DEMO_MODE,
    isWalletConnected,
    selectedProvisionServiceId,
    provisionExecutionTarget,
    chainSwitchBusy,
    txStatus,
    stalePendingTx,
  ]);
  const wizardAccessReady = hasToken && (DEMO_MODE || (isWalletConnected && !isWrongChain));
  const selectedAccessTokenDraft = selectedInstance ? (instanceAccessTokenInput[selectedInstance.id] ?? '') : '';
  const scopedSessionExpiryLabel =
    selectedScopedSession && scopedSessionIsValid
      ? new Date(selectedScopedSession.expiresAt * 1000).toLocaleTimeString()
      : 'not issued';
  const hasInjectedWalletProvider = Boolean(browserEthereum());
  const clearStalePendingTx = useCallback(() => {
    resetLifecycleTx();
    setPendingTxSinceMs(null);
    setNotice({ tone: 'info', text: 'Cleared stale pending transaction state.' });
  }, [resetLifecycleTx]);

  useEffect(() => {
    if (!notice) return;
    if (lastNoticeRef.current === notice) return;
    lastNoticeRef.current = notice;

    if (pendingSessionDelete) {
      toast.info(notice.text, {
        id: 'pending-session-delete',
        duration: 8_500,
        action: {
          label: 'Undo',
          onClick: undoPendingSessionDelete,
        },
      });
      setNotice(null);
      return;
    }

    if (notice.tone === 'success') {
      toast.success(notice.text);
    } else if (notice.tone === 'error') {
      toast.error(notice.text, { duration: 6_000 });
    } else {
      toast.info(notice.text);
    }
    setNotice(null);
  }, [notice, pendingSessionDelete, undoPendingSessionDelete]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const params = new URLSearchParams(window.location.search);
    params.set('view', surfaceTab);
    if (selectedId) params.set('instance', selectedId);
    else params.delete('instance');
    if (surfaceTab === 'workspace') params.set('panel', tab);
    else params.delete('panel');
    if (surfaceTab === 'launch' && wizardOpen) params.set('step', String(wizardStep));
    else params.delete('step');
    if (activeSessionId) params.set('session', activeSessionId);
    else params.delete('session');
    const next = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState(null, '', next);
  }, [activeSessionId, selectedId, surfaceTab, tab, wizardOpen, wizardStep]);

  return (
    <div className="min-h-screen claw-app-bg text-claw-elements-textPrimary">
      <AppToaster tone="cloud" />
      <AnimatedPage className="mx-auto max-w-6xl px-4 pb-8 pt-3 sm:px-6 space-y-3">
        <header className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <p className="font-display text-lg leading-tight">Claw Provisioning Console</p>
              <p className="text-xs text-claw-elements-textTertiary">Build {UI_BUILD_MARKER.slice(0, 19)}</p>
            </div>

            {isWalletConnected ? (
              <div className="relative">
                <button
                  ref={walletMenuButtonRef}
                  id="wallet-menu-trigger"
                  type="button"
                  aria-haspopup="menu"
                  aria-expanded={walletMenuOpen}
                  aria-controls="wallet-menu"
                  onClick={() => setWalletMenuOpen((current) => !current)}
                  className="h-8 rounded-lg border border-claw-elements-dividerColor bg-claw-elements-background-depth-2/80 px-2.5 text-[11px] text-claw-elements-textPrimary flex items-center gap-2 hover:bg-claw-elements-item-backgroundHover"
                >
                  <span className="i-ph:wallet text-sm claw-text-accent" aria-hidden="true" />
                  <span>{walletLabel}</span>
                  <span
                    className={cn(
                      'i-ph:caret-down text-xs text-claw-elements-textTertiary transition-transform',
                      walletMenuOpen ? 'rotate-180' : '',
                    )}
                    aria-hidden="true"
                  />
                </button>
                {walletMenuOpen ? (
                  <div
                    ref={walletMenuRef}
                    id="wallet-menu"
                    role="menu"
                    aria-labelledby="wallet-menu-trigger"
                    className="absolute right-0 top-full z-40 mt-2 w-[290px] rounded-xl border border-claw-elements-dividerColor bg-claw-elements-background-depth-2 p-3 shadow-[0_18px_34px_rgba(2,6,23,0.45)] space-y-3"
                  >
                    <div role="none">
                      <p className="text-[11px] text-claw-elements-textTertiary">Wallet Address</p>
                      <p className="font-data text-xs break-all">{connectedWallet}</p>
                    </div>
                    <div className="grid grid-cols-2 gap-2" role="none">
                      <div className="rounded-lg border border-claw-elements-dividerColor px-2 py-1.5">
                        <p className="text-[10px] text-claw-elements-textTertiary">Balance</p>
                        <p className="text-xs">{walletBalanceLabel}</p>
                      </div>
                      <div className="rounded-lg border border-claw-elements-dividerColor px-2 py-1.5">
                        <p className="text-[10px] text-claw-elements-textTertiary">Chain</p>
                        <p className="text-xs">{chainLabel}</p>
                      </div>
                    </div>
                    {walletBalanceError ? (
                      <p className="text-[11px] claw-text-warning" role="none">
                        Wallet RPC fallback active.
                      </p>
                    ) : null}
                    {walletBalanceTargetRpcHex ? (
                      <p className="text-[11px] text-claw-elements-textTertiary" role="none">
                        Target RPC balance check active ({TARGET_RPC_URL})
                      </p>
                    ) : null}
                    {isWrongChain ? (
                      <div className="rounded-lg border border-amber-400/35 bg-amber-500/10 px-2.5 py-2 space-y-2" role="none">
                        <p className="text-xs claw-text-warning">
                          Wrong chain selected. Switch to {TARGET_CHAIN_ID}.
                        </p>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => void ensureTargetChain({ attemptSwitch: true })}
                          disabled={chainSwitchBusy}
                        >
                          {isSwitchingChain ? 'Switching…' : `Switch to ${TARGET_CHAIN_ID}`}
                        </Button>
                      </div>
                    ) : null}
                    <div className="flex items-center gap-2" role="none">
                      <Button size="sm" variant="secondary" onClick={() => void copyWalletAddress()}>
                        {walletCopied ? 'Copied' : 'Copy Address'}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => {
                          setWalletMenuOpen(false);
                          disconnect();
                        }}
                      >
                        Disconnect
                      </Button>
                    </div>
                  </div>
                ) : null}
              </div>
            ) : (
              <Button
                size="sm"
                variant="secondary"
                onClick={() => void connectWallet()}
                disabled={connectors.length === 0 || isWalletConnectPending}
              >
                {isWalletConnectPending ? 'Connecting…' : 'Connect Wallet'}
              </Button>
            )}
          </div>

          <div className="flex items-center justify-start">
            <div className="inline-flex rounded-lg border border-claw-elements-dividerColor bg-claw-elements-background-depth-2/70 p-0.5">
              {(['launch', 'instances', 'workspace'] as SurfaceTab[]).map((id) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setSurfaceTab(id)}
                  className={cn(
                    'h-7 rounded-md px-3 text-[11px] font-medium capitalize transition-colors',
                    surfaceTab === id
                      ? 'bg-claw-elements-item-backgroundActive text-claw-elements-textPrimary'
                      : 'text-claw-elements-textSecondary hover:bg-claw-elements-item-backgroundHover',
                  )}
                >
                  {id}
                </button>
              ))}
            </div>
          </div>
        </header>

        {surfaceTab === 'launch' ? (
          <section className="space-y-4">
            {!wizardOpen ? (
              <Card className="glass">
                <CardHeader className="pb-3">
                  <CardTitle className="text-lg">Choose a Claw Variant</CardTitle>
                  <CardDescription>Select a runtime profile to start provisioning.</CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="grid gap-4 lg:grid-cols-3">
                    {(['openclaw', 'nanoclaw', 'ironclaw'] as ClawVariant[]).map((variant) => {
                      const view = VARIANT_PRESENTATION[variant];
                      return (
                        <button
                          key={variant}
                          type="button"
                          data-variant={view.tone}
                          onClick={() => startProvisionFlow(variant)}
                          className={cn('variant-card text-left', provisionVariant === variant && wizardOpen ? 'variant-card-active' : '')}
                        >
                          <div className="variant-figure">
                            <img
                              src={view.art}
                              width={640}
                              height={360}
                              loading="lazy"
                              alt={`${prettyVariantName(variant)} runtime preview`}
                              className="h-full w-full object-cover"
                            />
                          </div>
                          <div className="mt-3 flex items-center justify-between gap-2">
                            <p className="font-display text-base">{prettyVariantName(variant)}</p>
                            <Badge variant="secondary">{view.badge}</Badge>
                          </div>
                          <p className="mt-1 text-sm text-claw-elements-textSecondary">{view.subtitle}</p>
                          <div className="mt-3 space-y-1">
                            {view.bullets.map((bullet) => (
                              <p key={bullet} className="flex items-center gap-1.5 text-xs text-claw-elements-textTertiary">
                                <span className="i-ph:check-circle text-emerald-300" aria-hidden="true" />
                                <span>{bullet}</span>
                              </p>
                            ))}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </CardContent>
              </Card>
            ) : null}

            {wizardOpen ? (
              <Card className="glass">
                <CardHeader className="pb-3">
                  <div>
                    <CardTitle>{prettyVariantName(provisionVariant)} Provision Wizard</CardTitle>
                    <CardDescription>Complete profile, access, and submit.</CardDescription>
                  </div>
                </CardHeader>
                <CardContent className="space-y-4 min-h-[560px]">
                  <div className="grid grid-cols-3 gap-2">
                    <button
                      type="button"
                      onClick={() => setWizardStep(1)}
                      className={cn(
                        'rounded-lg border px-3 py-2 text-left text-sm',
                        wizardStep === 1
                          ? 'border-emerald-300/45 bg-emerald-500/14'
                          : 'border-claw-elements-dividerColor hover:bg-claw-elements-item-backgroundHover',
                      )}
                    >
                      1. Profile
                    </button>
                    <button
                      type="button"
                      onClick={() => setWizardStep(2)}
                      className={cn(
                        'rounded-lg border px-3 py-2 text-left text-sm',
                        wizardStep === 2
                          ? 'border-emerald-300/45 bg-emerald-500/14'
                          : 'border-claw-elements-dividerColor hover:bg-claw-elements-item-backgroundHover',
                      )}
                    >
                      2. Access
                    </button>
                    <button
                      type="button"
                      onClick={() => setWizardStep(3)}
                      className={cn(
                        'rounded-lg border px-3 py-2 text-left text-sm',
                        wizardStep === 3
                          ? 'border-emerald-300/45 bg-emerald-500/14'
                          : 'border-claw-elements-dividerColor hover:bg-claw-elements-item-backgroundHover',
                      )}
                    >
                      3. Submit
                    </button>
                  </div>
                  {launchFeedback ? (
                    <div
                      className={cn(
                        'rounded-lg border px-3 py-2 text-xs',
                        launchFeedback.tone === 'error'
                          ? 'border-red-400/45 bg-red-500/10 text-red-100'
                          : launchFeedback.tone === 'success'
                            ? 'border-emerald-300/40 bg-emerald-500/10 text-emerald-100'
                            : 'border-cyan-300/35 bg-cyan-500/10 text-cyan-100',
                      )}
                    >
                      {launchFeedback.text}
                    </div>
                  ) : null}

                  {wizardStep === 1 ? (
                    <div className="wizard-step-shell">
                      <div className="wizard-step-body space-y-4">
                        <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2 flex items-center justify-between gap-2">
                          <div>
                            <p className="text-xs text-claw-elements-textTertiary">Selected Variant</p>
                            <p className="text-sm">{prettyVariantName(provisionVariant)}</p>
                          </div>
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => {
                              setWizardOpen(false);
                              setWizardStep(1);
                            }}
                          >
                            Change
                          </Button>
                        </div>
                        <div className="grid gap-3 sm:grid-cols-2">
                          <div className="space-y-1">
                            <label htmlFor="instance_name" className="text-xs text-claw-elements-textTertiary">Instance Name</label>
                            <Input
                              id="instance_name"
                              name="instance_name"
                              autoComplete="off"
                              value={provisionName}
                              onChange={(event) => setProvisionName(event.target.value)}
                              placeholder="openclaw-xxxx…"
                            />
                          </div>
                          <div className="space-y-1">
                            <label htmlFor="public_subdomain" className="text-xs text-claw-elements-textTertiary">Public Subdomain</label>
                            <Input
                              id="public_subdomain"
                              name="public_subdomain"
                              autoComplete="off"
                              value={provisionSubdomain}
                              onChange={(event) => setProvisionSubdomain(event.target.value)}
                              placeholder="openclaw-xxxx…"
                            />
                          </div>
                        </div>
                        <div className="space-y-2">
                          <div className="flex items-center justify-between gap-2">
                            <label className="text-xs text-claw-elements-textTertiary">Template Profile</label>
                            <Badge variant="secondary">
                              {selectedTemplate ? prettyTemplateMode(selectedTemplate.mode) : 'Default'}
                            </Badge>
                          </div>
                          <div className="grid gap-2 sm:grid-cols-2">
                            {templates.length === 0 ? (
                              <button
                                type="button"
                                onClick={() => setProvisionTemplateId('ops')}
                                className="template-card template-card-active text-left"
                              >
                                <div className="flex items-center justify-between gap-2">
                                  <p className="text-sm font-medium">ops</p>
                                  <Badge variant="secondary">Default</Badge>
                                </div>
                                <p className="mt-1 text-xs text-claw-elements-textSecondary">
                                  Standard runtime profile for local development.
                                </p>
                                <p className="mt-2 text-[11px] font-data text-claw-elements-textTertiary">id: ops</p>
                              </button>
                            ) : (
                              templates.map((pack) => {
                                const isSelected = pack.id === provisionTemplateId;
                                return (
                                  <button
                                    key={pack.id}
                                    type="button"
                                    onClick={() => setProvisionTemplateId(pack.id)}
                                    className={cn('template-card text-left', isSelected ? 'template-card-active' : '')}
                                  >
                                    <div className="flex items-center justify-between gap-2">
                                      <p className="text-sm font-medium">{pack.name || pack.id}</p>
                                      <Badge variant={isSelected ? 'success' : 'secondary'}>
                                        {prettyTemplateMode(pack.mode)}
                                      </Badge>
                                    </div>
                                    <p className="mt-1 text-xs text-claw-elements-textSecondary">
                                      {pack.description || 'Runtime profile without custom description.'}
                                    </p>
                                    <p className="mt-2 text-[11px] font-data text-claw-elements-textTertiary">
                                      id: {pack.id}
                                    </p>
                                  </button>
                                );
                              })
                            )}
                          </div>
                        </div>
                        <div className="space-y-1 min-w-[180px]">
                          <label htmlFor="execution_target" className="text-xs text-claw-elements-textTertiary">Execution Target</label>
                          <select
                            id="execution_target"
                            value={provisionExecutionTarget}
                            onChange={(event) => setProvisionExecutionTarget(event.target.value as 'standard' | 'tee')}
                            className="h-10 w-full rounded-md border border-claw-elements-dividerColor bg-claw-elements-background-depth-2 px-3 text-sm"
                          >
                            <option value="standard">Standard</option>
                            <option value="tee">TEE</option>
                          </select>
                        </div>
                      </div>
                      <div className="wizard-actions">
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => {
                            setWizardOpen(false);
                            setWizardStep(1);
                          }}
                        >
                          Back
                        </Button>
                        <div className="wizard-actions-right">
                          <Button size="sm" variant="secondary" onClick={regenerateProvisionIdentity}>
                            Regenerate IDs
                          </Button>
                          <Button size="sm" onClick={() => setWizardStep(2)} disabled={!canProvision}>
                            Continue
                          </Button>
                        </div>
                      </div>
                    </div>
                  ) : null}

                  {wizardStep === 2 ? (
                    <div className="wizard-step-shell">
                      <div className="wizard-step-body space-y-4">
                        <div className="rounded-xl border border-claw-elements-dividerColor px-3 py-3">
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div className="flex items-center gap-2">
                              <span className="i-ph:wallet text-base text-emerald-300" aria-hidden="true" />
                              <div>
                                <p className="text-sm">Wallet Signature Access</p>
                                <p className="text-xs text-claw-elements-textTertiary">
                                  {activeChainId ? `${walletLabel} · ${chainLabel}` : 'Connect wallet to sign create job.'}
                                </p>
                              </div>
                            </div>
                            {isWalletConnected && isWrongChain ? (
                              <Button
                                size="sm"
                                variant="secondary"
                                onClick={() => void ensureTargetChain({ attemptSwitch: true })}
                                disabled={chainSwitchBusy}
                              >
                                {isSwitchingChain ? 'Switching…' : `Switch to ${TARGET_CHAIN_ID}`}
                              </Button>
                            ) : isWalletConnected ? (
                              <Button size="sm" variant="secondary" onClick={() => disconnect()}>
                                Disconnect
                              </Button>
                            ) : (
                              <Button
                                size="sm"
                                variant="secondary"
                                onClick={() => void connectWallet()}
                                disabled={connectors.length === 0 || isWalletConnectPending}
                              >
                                {isWalletConnectPending ? 'Connecting…' : 'Connect Wallet'}
                              </Button>
                            )}
                          </div>
                        </div>
                        {staleSwitchRequest ? (
                          <p className="text-xs claw-text-warning">
                            Chain switch request is still pending in wallet for {Math.round(switchingAgeMs / 1000)}s.
                            Approve or reject it in MetaMask, then click switch again.
                          </p>
                        ) : null}

                        <div className="rounded-xl border border-claw-elements-dividerColor px-3 py-3 space-y-2">
                          <div className="flex items-center justify-between gap-2">
                            <label htmlFor="owner_api_token_wizard" className="text-xs text-claw-elements-textTertiary">Operator Bearer Token</label>
                            <Badge variant={hasToken ? 'success' : 'amber'}>{hasToken ? 'Saved' : 'Missing'}</Badge>
                          </div>
                          <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
                            <Input
                              type="password"
                              id="owner_api_token_wizard"
                              name="owner_api_token_wizard"
                              autoComplete="off"
                              spellCheck={false}
                              value={tokenInput}
                              onChange={(event) => setTokenInput(event.target.value)}
                              placeholder="oclw_…"
                            />
                            <Button size="sm" onClick={onSaveToken}>Save Token</Button>
                            <Button size="sm" variant="ghost" onClick={onUseDemoToken}>Use Local Dev Token</Button>
                          </div>
                          <div className="flex flex-wrap items-center gap-2">
                            <p className="text-xs text-claw-elements-textTertiary">
                              Saved {previewToken(token)} · Draft {previewToken(tokenInput.trim())}
                            </p>
                          </div>
                        </div>
                      </div>
                      <div className="wizard-actions">
                        <Button size="sm" variant="ghost" onClick={() => setWizardStep(1)}>Back</Button>
                        <div className="wizard-actions-right">
                          <Button size="sm" onClick={() => setWizardStep(3)} disabled={!wizardAccessReady}>
                            Continue
                          </Button>
                        </div>
                      </div>
                    </div>
                  ) : null}

                  {wizardStep === 3 ? (
                    <div className="wizard-step-shell">
                      <div className="wizard-step-body space-y-4">
                        <div className="rounded-xl border border-claw-elements-dividerColor px-3 py-3 space-y-3">
                          <div className="flex items-center gap-2">
                            <span className="i-ph:sparkle text-emerald-300" aria-hidden="true" />
                            <p className="font-display text-base">Ready to Create</p>
                          </div>
                          <p className="text-sm text-claw-elements-textSecondary">
                            Final check before submitting the create job.
                          </p>
                          <div className="grid gap-2 sm:grid-cols-3">
                            <div className="rounded-lg border border-claw-elements-dividerColor px-2.5 py-2">
                              <p className="text-[11px] text-claw-elements-textTertiary">Profile</p>
                              <p className="text-xs">{canProvision ? 'Ready' : 'Incomplete'}</p>
                            </div>
                            <div className="rounded-lg border border-claw-elements-dividerColor px-2.5 py-2">
                              <p className="text-[11px] text-claw-elements-textTertiary">Wallet</p>
                              <p className="text-xs">
                                {DEMO_MODE
                                  ? 'Not required'
                                  : isWalletConnected && !isWrongChain
                                    ? 'Ready'
                                    : isWalletConnected
                                      ? `Wrong chain (${activeChainId ?? 'n/a'})`
                                      : 'Missing'}
                              </p>
                            </div>
                            <div className="rounded-lg border border-claw-elements-dividerColor px-2.5 py-2">
                              <p className="text-[11px] text-claw-elements-textTertiary">Token</p>
                              <p className="text-xs">{hasToken ? 'Ready' : 'Missing'}</p>
                            </div>
                          </div>
                        </div>

                        <div className="grid gap-2 sm:grid-cols-2">
                          <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                            <p className="text-[11px] text-claw-elements-textTertiary">Variant</p>
                            <p className="text-sm">{prettyVariantName(provisionVariant)}</p>
                          </div>
                          <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                            <p className="text-[11px] text-claw-elements-textTertiary">Execution Target</p>
                            <p className="text-sm">{provisionExecutionTarget}</p>
                          </div>
                          <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                            <p className="text-[11px] text-claw-elements-textTertiary">Instance Name</p>
                            <p className="text-sm truncate">{provisionName}</p>
                          </div>
                          <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                            <p className="text-[11px] text-claw-elements-textTertiary">Subdomain</p>
                            <p className="text-sm truncate">{provisionSubdomain}</p>
                          </div>
                        </div>

                        <details className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                          <summary className="cursor-pointer text-sm text-claw-elements-textSecondary">Advanced Routing</summary>
                          <div className="mt-3 grid gap-3 sm:grid-cols-2">
                            <div className="space-y-1">
                              <label htmlFor="standard_service_id" className="text-xs text-claw-elements-textTertiary">Standard Service ID</label>
                              <Input
                                id="standard_service_id"
                                value={standardServiceIdInput}
                                onChange={(event) => setStandardServiceIdInput(event.target.value)}
                                placeholder="service-id"
                                className="font-data"
                              />
                            </div>
                            <div className="space-y-1">
                              <label htmlFor="tee_service_id" className="text-xs text-claw-elements-textTertiary">TEE Service ID</label>
                              <Input
                                id="tee_service_id"
                                value={teeServiceIdInput}
                                onChange={(event) => setTeeServiceIdInput(event.target.value)}
                                placeholder="service-id"
                                className="font-data"
                              />
                            </div>
                          </div>
                        </details>

                        {!DEMO_MODE && txHash ? (
                          <p className="text-xs text-claw-elements-textTertiary break-all">Last Tx {txHash}</p>
                        ) : null}
                        {txError ? <p className="text-xs claw-text-danger">Transaction error: {txError}</p> : null}
                        {!DEMO_MODE && stalePendingTx ? (
                          <div className="flex flex-wrap items-center gap-2">
                            <p className="text-xs claw-text-warning">
                              Previous transaction appears stale ({Math.round(pendingTxAgeMs / 1000)}s without receipt).
                            </p>
                            <Button size="sm" variant="ghost" onClick={clearStalePendingTx}>
                              Clear Pending State
                            </Button>
                          </div>
                        ) : null}
                      </div>
                      <div className="wizard-actions">
                        <Button size="sm" variant="ghost" onClick={() => setWizardStep(2)}>Back</Button>
                        <div className="wizard-actions-right">
                          <Button
                            onClick={() => void onProvisionInstance()}
                            disabled={
                              isProvisioning ||
                              !canProvision ||
                              (!DEMO_MODE && (!isWalletConnected || selectedProvisionServiceId === null || lifecycleTxBusy))
                            }
                          >
                            {isProvisioning
                              ? 'Submitting…'
                              : DEMO_MODE
                                ? 'Create Instance'
                                : lifecycleTxBusy
                                  ? 'Transaction Pending…'
                                  : 'Submit Create Job'}
                          </Button>
                        </div>
                      </div>
                      {!DEMO_MODE && createSubmitDisabledReason ? (
                        <p className="text-xs text-claw-elements-textTertiary">{createSubmitDisabledReason}</p>
                      ) : null}
                    </div>
                  ) : null}
                </CardContent>
              </Card>
            ) : null}
          </section>
        ) : null}

        {surfaceTab === 'instances' ? (
          <Card className="glass">
            <CardHeader className="pb-3">
              <CardTitle>Instances</CardTitle>
              <CardDescription>Select an instance to open workspace controls.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <Badge variant="secondary">Total {totalInstances}</Badge>
                <Badge variant="secondary">Running {runningInstances}</Badge>
                <Badge variant="secondary">TEE {teeInstances}</Badge>
                <Button size="sm" variant="ghost" onClick={() => void refresh()} disabled={isRefreshing || !hasToken}>
                  {isRefreshing ? 'Refreshing…' : 'Refresh'}
                </Button>
              </div>
              <div className="max-h-[min(560px,62dvh)] space-y-2 overflow-y-auto pr-1 scroll-thin">
                {instances.length === 0 ? (
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-4 text-sm text-claw-elements-textSecondary">
                    No instances found for this session.
                  </div>
                ) : (
                  instances.map((instance) => (
                    <button
                      key={instance.id}
                      type="button"
                      onClick={() => {
                        setSelectedId(instance.id);
                        setSurfaceTab('workspace');
                      }}
                      className={cn(
                        'w-full rounded-lg border p-3 text-left transition-colors',
                        selectedId === instance.id
                          ? 'border-emerald-400/55 bg-emerald-500/12'
                          : 'border-claw-elements-dividerColor hover:bg-claw-elements-item-backgroundHover',
                      )}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <p className="font-display text-sm truncate">{instance.name}</p>
                        <Badge variant={statusTone(instance.status)}>{instance.status}</Badge>
                      </div>
                      <div className="mt-2 flex flex-wrap gap-2 text-xs">
                        <Badge variant="secondary">{instance.clawVariant}</Badge>
                        <Badge variant={connectedWallet && sameAddress(instance.owner, connectedWallet) ? 'success' : 'secondary'}>
                          {connectedWallet && sameAddress(instance.owner, connectedWallet) ? 'owned' : 'external'}
                        </Badge>
                        <Badge variant={instance.executionTarget === 'tee' ? 'amber' : 'secondary'}>
                          {instance.executionTarget}
                        </Badge>
                        <Badge variant={instance.uiAccess.tunnelStatus === 'active' ? 'success' : 'secondary'}>
                          tunnel {instance.uiAccess.tunnelStatus}
                        </Badge>
                      </div>
                      <p className="mt-2 text-[11px] text-claw-elements-textTertiary font-data truncate">{instance.id}</p>
                    </button>
                  ))
                )}
              </div>
            </CardContent>
          </Card>
        ) : null}

        {surfaceTab === 'workspace' ? (
          selectedInstance ? (
            <Card className="glass">
              <CardHeader className="pb-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <CardTitle>{selectedInstance.name}</CardTitle>
                    <CardDescription>
                      {selectedInstance.clawVariant} · {selectedInstance.executionTarget} · created {formatDate(selectedInstance.createdAt)}
                    </CardDescription>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge variant={statusTone(selectedInstance.status)}>{selectedInstance.status}</Badge>
                    <Badge variant={selectedInstanceOwnedByWallet ? 'success' : 'secondary'}>
                      {selectedInstanceOwnedByWallet ? 'owned' : 'external'}
                    </Badge>
                    <Badge variant="secondary">service {selectedInstanceServiceId?.toString() ?? 'missing'}</Badge>
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void onSubmitLifecycleJob(JOB_START, 'Start Instance')}
                    disabled={
                      lifecycleTxBusy ||
                      !isWalletConnected ||
                      !selectedInstanceOwnedByWallet ||
                      selectedInstance.status !== 'stopped'
                    }
                  >
                    Start
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void onSubmitLifecycleJob(JOB_STOP, 'Stop Instance')}
                    disabled={
                      lifecycleTxBusy ||
                      !isWalletConnected ||
                      !selectedInstanceOwnedByWallet ||
                      selectedInstance.status !== 'running'
                    }
                  >
                    Stop
                  </Button>
                  <Button
                    size="sm"
                    variant="destructive"
                    onClick={() => void confirmAndDeleteInstance()}
                    disabled={
                      lifecycleTxBusy ||
                      !isWalletConnected ||
                      !selectedInstanceOwnedByWallet ||
                      selectedInstance.status === 'deleted'
                    }
                  >
                    Delete
                  </Button>
                  <Button
                    size="sm"
                    onClick={() => void onOneClickSetup()}
                    disabled={isSetupStarting || selectedInstance.status !== 'running' || !selectedInstanceSetupCapable}
                  >
                    {isSetupStarting ? 'Starting Setup…' : 'Start Setup'}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void onFetchInstanceAccess()}
                    disabled={!selectedInstanceAccessCapable}
                  >
                    Fetch Access
                  </Button>
                </div>
                {workspaceFeedback ? (
                  <div
                    className={cn(
                      'rounded-lg border px-3 py-2 text-xs',
                      workspaceFeedback.tone === 'error'
                        ? 'border-red-400/45 bg-red-500/10 text-red-100'
                        : workspaceFeedback.tone === 'success'
                          ? 'border-emerald-300/40 bg-emerald-500/10 text-emerald-100'
                          : 'border-cyan-300/35 bg-cyan-500/10 text-cyan-100',
                    )}
                  >
                    {workspaceFeedback.text}
                  </div>
                ) : null}
                {!hasScopedSession ? (
                  <p className="text-xs text-claw-elements-textTertiary">
                    Setup actions auto-create a scoped owner session. If wallet signing is blocked, refresh owner session first.
                  </p>
                ) : null}

                <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-3 space-y-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <p className="text-sm">Owner Session</p>
                      <p className="text-xs text-claw-elements-textTertiary">
                        Auth mode: {selectedAuthMode} · session {hasScopedSession ? 'ready' : 'required'}
                      </p>
                    </div>
                    <Button
                      size="sm"
                      variant={hasScopedSession ? 'secondary' : 'default'}
                      onClick={() => void onCreateScopedSession()}
                      disabled={isCreatingScopedSession}
                    >
                      {isCreatingScopedSession
                        ? 'Authorizing…'
                        : selectedAuthMode === 'wallet_signature'
                          ? hasScopedSession
                            ? 'Refresh Owner Session'
                            : 'Create Owner Session'
                          : hasScopedSession
                            ? 'Refresh Access Session'
                            : 'Create Access Session'}
                    </Button>
                  </div>
                  {selectedAuthMode === 'wallet_signature' && !isWalletConnected ? (
                    <p className="text-xs claw-text-warning">Connect the owner wallet to create a scoped session.</p>
                  ) : null}
                  {selectedAuthMode === 'wallet_signature' &&
                  isWalletConnected &&
                  !selectedInstanceOwnedByWallet ? (
                    <p className="text-xs claw-text-warning">
                      This instance is owned by {truncateAddress(selectedInstance.owner)}. Select an owned instance.
                    </p>
                  ) : null}
                  {selectedAuthMode === 'wallet_signature' &&
                  isWalletConnected &&
                  !hasInjectedWalletProvider ? (
                    <p className="text-xs claw-text-warning">
                      Wallet provider is unavailable on this origin.
                      {DEV_INSTANCE_ACCESS_TOKEN
                        ? ' Using local access-token fallback for owner session bootstrap.'
                        : ' Open this UI on https/localhost (or reconnect wallet) to enable signature-based owner sessions.'}
                    </p>
                  ) : null}
                  {selectedAuthMode === 'access_token' ? (
                    <div className="space-y-1">
                      <label htmlFor="instance_access_token" className="text-xs text-claw-elements-textTertiary">
                        Instance Access Token
                      </label>
                      <Input
                        id="instance_access_token"
                        type="password"
                        value={selectedAccessTokenDraft}
                        onChange={(event) =>
                          setInstanceAccessTokenInput((current) => ({
                            ...current,
                            [selectedInstance.id]: event.target.value,
                          }))
                        }
                        placeholder="instance access token"
                      />
                    </div>
                  ) : null}
                  <p className="text-xs text-claw-elements-textTertiary">
                    Session expires: {scopedSessionExpiryLabel}
                  </p>
                  {ownerSessionFeedback ? (
                    <p
                      className={
                        ownerSessionFeedback.tone === 'error'
                          ? 'text-xs claw-text-danger'
                          : ownerSessionFeedback.tone === 'success'
                            ? 'text-xs claw-text-success'
                            : 'text-xs text-claw-elements-textTertiary'
                      }
                    >
                      {ownerSessionFeedback.text}
                    </p>
                  ) : null}
                </div>

                <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                    <p className="text-xs text-claw-elements-textTertiary">Public URL</p>
                    <p className="text-xs font-data truncate">{selectedInstance.uiAccess.publicUrl ?? 'pending'}</p>
                  </div>
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                    <p className="text-xs text-claw-elements-textTertiary">Tunnel</p>
                    <p className="text-sm">{selectedInstance.uiAccess.tunnelStatus}</p>
                  </div>
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                    <p className="text-xs text-claw-elements-textTertiary">Setup status</p>
                    <p className="text-sm">{selectedInstance.runtime.setupStatus ?? 'n/a'}</p>
                  </div>
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-2">
                    <p className="text-xs text-claw-elements-textTertiary">Setup URL</p>
                    <p className="text-xs font-data truncate">{selectedInstance.runtime.setupUrl ?? 'n/a'}</p>
                  </div>
                </div>
                {connectedWallet && !selectedInstanceOwnedByWallet ? (
                  <div className="rounded-lg border border-amber-400/35 bg-amber-500/10 px-3 py-2 text-xs claw-text-warning">
                    Connected wallet does not own this instance. Lifecycle transactions are disabled.
                  </div>
                ) : null}

                {instanceAccess ? (
                  <div className="rounded-lg border border-claw-elements-dividerColor px-3 py-3 text-xs font-data space-y-1">
                    <p className="text-claw-elements-textTertiary">Instance bearer token</p>
                    <p className="break-all">{instanceAccess.bearerToken}</p>
                    <p className="text-claw-elements-textTertiary">Public URL: {instanceAccess.publicUrl ?? 'n/a'}</p>
                  </div>
                ) : null}

                <Tabs value={tab} onValueChange={(value) => setTab(value as MainTab)}>
                  <TabsList className="grid grid-cols-2 sm:grid-cols-4 gap-1">
                    <TabsTrigger value="workspace">Setup</TabsTrigger>
                    <TabsTrigger value="terminal">Terminal</TabsTrigger>
                    <TabsTrigger value="chat">Chat</TabsTrigger>
                    <TabsTrigger value="advanced">Advanced</TabsTrigger>
                  </TabsList>

                  <TabsContent value="workspace" className="pt-4 space-y-3">
                    <label htmlFor="setup_env_overrides" className="text-xs text-claw-elements-textTertiary">Setup env overrides (optional)</label>
                    <Textarea
                      id="setup_env_overrides"
                      value={setupEnvText}
                      onChange={(event) => setSetupEnvText(event.target.value)}
                      placeholder={'OPENCLAW_THEME=night\nOPENCLAW_REGION=us-west'}
                      className="min-h-28 font-data"
                    />
                    <Button
                      variant="secondary"
                      onClick={() => void onSetupWithEnv()}
                      disabled={isSetupStarting || selectedInstance.status !== 'running' || !selectedInstanceSetupCapable}
                    >
                      {isSetupStarting ? 'Starting setup…' : 'Start setup with env'}
                    </Button>
                    {!selectedInstanceSetupCapable ? (
                      <p className="text-xs claw-text-warning">
                        Setup not supported on current runtime backend. Use docker runtime for setup execution.
                      </p>
                    ) : null}
                  </TabsContent>

                  <TabsContent value="terminal" className="pt-4">
                    <div className="h-[min(560px,68dvh)] min-h-[360px] rounded-xl border border-claw-elements-dividerColor overflow-hidden bg-[#070d15]">
                      {scopedToken ? (
                        <Suspense fallback={<div className="p-4 text-sm">Loading Terminal…</div>}>
                          <TerminalView apiUrl={selectedApiBase} token={scopedToken} title="Runtime Terminal" subtitle="Scoped shell" />
                        </Suspense>
                      ) : (
                        <div className="p-6 text-sm text-claw-elements-textSecondary">
                          Create an owner session to open terminal access.
                        </div>
                      )}
                    </div>
                  </TabsContent>

                  <TabsContent value="chat" className="pt-4">
                    <div className="grid gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
                      <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                        <CardHeader>
                          <CardTitle className="text-sm">Sessions</CardTitle>
                        </CardHeader>
                        <CardContent className="space-y-2 max-h-[min(520px,64dvh)] overflow-y-auto scroll-thin">
                          <Button
                            variant="secondary"
                            className="w-full"
                            onClick={() =>
                              createSessionMutation.mutate('Session', {
                                onSuccess: (session) => {
                                  setActiveSessionId(session.id);
                                  setNotice({ tone: 'success', text: 'Chat session created.' });
                                },
                                onError: (error) => {
                                  setNotice({ tone: 'error', text: `Session create failed: ${(error as Error).message}` });
                                },
                              })
                            }
                            disabled={!selectedApiBase || !scopedToken || createSessionMutation.isPending}
                          >
                            {createSessionMutation.isPending ? 'Creating…' : 'New Session'}
                          </Button>

                          {(sessions.data ?? []).map((session) => (
                            <div key={session.id} className="rounded-lg border border-claw-elements-dividerColor p-2">
                              <button
                                type="button"
                                className={cn(
                                  'w-full text-left text-sm truncate',
                                  activeSessionId === session.id ? 'claw-text-accent' : 'text-claw-elements-textPrimary',
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
                                  className="h-7 px-2 claw-text-danger"
                                  onClick={() => queueSessionDelete(session.id, session.title)}
                                  disabled={pendingSessionDelete?.id === session.id}
                                >
                                  {pendingSessionDelete?.id === session.id ? 'Pending…' : 'Delete'}
                                </Button>
                              </div>
                            </div>
                          ))}
                        </CardContent>
                      </Card>

                      <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2 min-h-[min(520px,64dvh)]">
                        <CardContent className="p-0 h-full min-h-[min(520px,64dvh)]">
                          {activeSessionId ? (
                            <ChatContainer
                              messages={sessionStream.messages}
                              partMap={sessionStream.partMap}
                              isStreaming={sessionStream.isStreaming}
                              onSend={(text) => {
                                void sessionStream.send(text);
                              }}
                              branding={CHAT_BRANDING}
                              placeholder="Send a command or ask for setup help."
                              className="h-[min(520px,64dvh)]"
                            />
                          ) : (
                            <div className="h-[min(520px,64dvh)] p-5 text-sm text-claw-elements-textSecondary">
                              Create a session to start chat.
                            </div>
                          )}
                        </CardContent>
                      </Card>
                    </div>
                  </TabsContent>

                  <TabsContent value="advanced" className="pt-4 space-y-4">
                    <div className="grid gap-4 xl:grid-cols-2">
                      <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                        <CardHeader>
                          <CardTitle className="text-sm">One-shot command</CardTitle>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          <label htmlFor="one_shot_command" className="text-xs text-claw-elements-textTertiary">Command</label>
                          <Input
                            id="one_shot_command"
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

                      <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                        <CardHeader>
                          <CardTitle className="text-sm">SSH key</CardTitle>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          <label htmlFor="ssh_username" className="text-xs text-claw-elements-textTertiary">Username</label>
                          <Input
                            id="ssh_username"
                            value={sshUsername}
                            onChange={(event) => setSshUsername(event.target.value)}
                            placeholder="agent"
                          />
                          <label htmlFor="ssh_public_key" className="text-xs text-claw-elements-textTertiary">Public key</label>
                          <Textarea
                            id="ssh_public_key"
                            value={sshPublicKey}
                            onChange={(event) => setSshPublicKey(event.target.value)}
                            placeholder="ssh-ed25519 AAAA..."
                            className="min-h-20 font-data"
                          />
                          <div className="flex gap-2">
                            <Button variant="secondary" onClick={() => void onSshUpsert('POST')}>Add Key</Button>
                            <Button variant="ghost" onClick={() => void onSshUpsert('DELETE')}>Revoke Key</Button>
                          </div>
                        </CardContent>
                      </Card>
                    </div>

                    <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                      <CardHeader>
                        <CardTitle className="text-sm">Quick assistant prompt</CardTitle>
                      </CardHeader>
                      <CardContent className="space-y-3">
                        <label htmlFor="quick_chat_prompt" className="text-xs text-claw-elements-textTertiary">Prompt</label>
                        <Input
                          id="quick_chat_prompt"
                          value={quickChatPrompt}
                          onChange={(event) => setQuickChatPrompt(event.target.value)}
                          placeholder="hello"
                        />
                        <Button variant="secondary" onClick={() => void onQuickChat()}>
                          Send Prompt
                        </Button>
                      </CardContent>
                    </Card>

                    {selectedInstance.executionTarget === 'tee' ? (
                      <Card className="border-claw-elements-dividerColor bg-claw-elements-background-depth-2">
                        <CardHeader>
                          <CardTitle className="text-sm">TEE</CardTitle>
                        </CardHeader>
                        <CardContent className="space-y-3">
                          <div className="grid gap-3 sm:grid-cols-3">
                            <Button variant="secondary" onClick={() => void onTeePublicKey()}>Public Key</Button>
                            <Button variant="secondary" onClick={() => void onTeeAttestation()}>Attestation</Button>
                            <Button variant="secondary" onClick={() => void onTeeSealedSecret()}>Sealed Secret</Button>
                          </div>
                          <label htmlFor="tee_algorithm" className="text-xs text-claw-elements-textTertiary">Algorithm</label>
                          <Input
                            id="tee_algorithm"
                            value={teeAlgorithm}
                            onChange={(event) => setTeeAlgorithm(event.target.value)}
                            placeholder="x25519-xsalsa20-poly1305"
                          />
                          <label htmlFor="tee_ciphertext" className="text-xs text-claw-elements-textTertiary">Ciphertext bytes</label>
                          <Textarea
                            id="tee_ciphertext"
                            value={teeCiphertext}
                            onChange={(event) => setTeeCiphertext(event.target.value)}
                            placeholder="0x010203"
                            className="min-h-20 font-data"
                          />
                          <label htmlFor="tee_nonce" className="text-xs text-claw-elements-textTertiary">Nonce bytes</label>
                          <Textarea
                            id="tee_nonce"
                            value={teeNonce}
                            onChange={(event) => setTeeNonce(event.target.value)}
                            placeholder="0x010203"
                            className="min-h-20 font-data"
                          />
                          <Textarea readOnly value={teeOutput} className="min-h-36 font-data" />
                        </CardContent>
                      </Card>
                    ) : null}
                  </TabsContent>
                </Tabs>
              </CardContent>
            </Card>
          ) : (
            <Card className="glass">
              <CardHeader>
                <CardTitle>No Selected Instance</CardTitle>
                <CardDescription>Select an instance from the Instances tab, or provision a new runtime.</CardDescription>
              </CardHeader>
              <CardContent className="flex flex-wrap gap-2">
                <Button variant="secondary" onClick={() => setSurfaceTab('instances')}>Open Instances</Button>
                <Button onClick={() => setSurfaceTab('launch')}>Open Launch</Button>
              </CardContent>
            </Card>
          )
        ) : null}
      </AnimatedPage>
    </div>
  );
}

export default function App() {
  return <InstanceRuntimePanel />;
}
