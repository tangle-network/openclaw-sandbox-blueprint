import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { type ReactNode, useEffect, useState } from 'react';
import { defineChain } from 'viem';
import { mainnet } from 'viem/chains';
import { WagmiProvider, createConfig, http, useReconnect } from 'wagmi';
import { injected, walletConnect } from 'wagmi/connectors';

const configuredRpcUrl = import.meta.env.VITE_RPC_URL?.trim();
const configuredWsRpcUrl = import.meta.env.VITE_WS_RPC_URL?.trim();
const defaultRpcPort = import.meta.env.VITE_RPC_PORT?.trim() || '8745';
const defaultWsRpcPort = import.meta.env.VITE_WS_RPC_PORT?.trim() || defaultRpcPort;
const localChainId = Number(import.meta.env.VITE_CHAIN_ID ?? 31337);

function isLoopbackHost(hostname: string): boolean {
  return hostname === 'localhost' || hostname === '127.0.0.1' || hostname === '::1';
}

function deriveWsFromHttp(url: string): string {
  if (url.startsWith('https://')) return `wss://${url.slice('https://'.length)}`;
  if (url.startsWith('http://')) return `ws://${url.slice('http://'.length)}`;
  return url;
}

function resolveLocalRpcUrls(): { httpUrl: string; wsUrl: string } {
  if (configuredRpcUrl) {
    return {
      httpUrl: configuredRpcUrl,
      wsUrl: configuredWsRpcUrl || deriveWsFromHttp(configuredRpcUrl),
    };
  }

  if (typeof window !== 'undefined') {
    const host = window.location.hostname;
    const isHttps = window.location.protocol === 'https:';
    const httpProtocol = isHttps ? 'https' : 'http';
    const wsProtocol = isHttps ? 'wss' : 'ws';
    return {
      httpUrl: `${httpProtocol}://${host}:${defaultRpcPort}`,
      wsUrl: `${wsProtocol}://${host}:${defaultWsRpcPort}`,
    };
  }

  return {
    httpUrl: `http://127.0.0.1:${defaultRpcPort}`,
    wsUrl: `ws://127.0.0.1:${defaultWsRpcPort}`,
  };
}

const { httpUrl: localRpcUrl, wsUrl: localWsRpcUrl } = resolveLocalRpcUrls();

if (typeof window !== 'undefined') {
  try {
    const pageHost = window.location.hostname;
    const rpcHost = new URL(localRpcUrl).hostname;
    if (!isLoopbackHost(pageHost) && isLoopbackHost(rpcHost)) {
      console.error(
        `[openclaw-ui] Invalid RPC config for remote page host ${pageHost}: local RPC resolves to loopback (${localRpcUrl}).`,
      );
    }
  } catch {
    // Ignore URL parsing failures and let wagmi surface transport errors.
  }
}

const tangleLocal = defineChain({
  id: localChainId,
  name: 'Tangle Local',
  nativeCurrency: { name: 'Ether', symbol: 'ETH', decimals: 18 },
  rpcUrls: { default: { http: [localRpcUrl], webSocket: [localWsRpcUrl] } },
  blockExplorers: { default: { name: 'Explorer', url: '' } },
  contracts: { multicall3: { address: '0xcA11bde05977b3631167028862bE2a173976CA11' } },
});

const tangleTestnet = defineChain({
  id: 3799,
  name: 'Tangle Testnet',
  nativeCurrency: { name: 'Tangle', symbol: 'tTNT', decimals: 18 },
  rpcUrls: {
    default: {
      http: ['https://testnet-rpc.tangle.tools'],
      webSocket: ['wss://testnet-rpc.tangle.tools'],
    },
  },
  blockExplorers: { default: { name: 'Tangle Explorer', url: 'https://testnet-explorer.tangle.tools' } },
  contracts: { multicall3: { address: '0xcA11bde05977b3631167028862bE2a173976CA11' } },
});

const tangleMainnet = defineChain({
  id: 5845,
  name: 'Tangle',
  nativeCurrency: { name: 'Tangle', symbol: 'TNT', decimals: 18 },
  rpcUrls: {
    default: {
      http: ['https://rpc.tangle.tools'],
      webSocket: ['wss://rpc.tangle.tools'],
    },
  },
  blockExplorers: { default: { name: 'Tangle Explorer', url: 'https://explorer.tangle.tools' } },
  contracts: { multicall3: { address: '0xcA11bde05977b3631167028862bE2a173976CA11' } },
});

const walletChains = [tangleLocal, tangleTestnet, tangleMainnet, mainnet] as const;

const walletConnectProjectId = import.meta.env.VITE_WALLETCONNECT_PROJECT_ID || '';
const connectors = walletConnectProjectId
  ? [
      injected({
        shimDisconnect: true,
      }),
      walletConnect({
        projectId: walletConnectProjectId,
        showQrModal: true,
      }),
    ]
  : [
      injected({
        shimDisconnect: true,
      }),
    ];

const config = createConfig({
  chains: walletChains,
  transports: {
    [tangleLocal.id]: http(localRpcUrl),
    [tangleTestnet.id]: http('https://testnet-rpc.tangle.tools'),
    [tangleMainnet.id]: http('https://rpc.tangle.tools'),
    [mainnet.id]: http(),
  },
  connectors,
  ssr: false,
});

function FastReconnect({ children }: { children: ReactNode }) {
  const { reconnect } = useReconnect();

  useEffect(() => {
    const injectedConnector = config.connectors.find((connector) => connector.type === 'injected');
    if (injectedConnector) {
      reconnect({ connectors: [injectedConnector] });
    }
  }, [reconnect]);

  return <>{children}</>;
}

export function Web3Provider({ children }: { children: ReactNode }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            refetchOnWindowFocus: false,
          },
        },
      }),
  );

  return (
    <WagmiProvider config={config}>
      <QueryClientProvider client={queryClient}>
        <FastReconnect>{children}</FastReconnect>
      </QueryClientProvider>
    </WagmiProvider>
  );
}
